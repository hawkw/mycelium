use crate::{
    task::{self, Header, Storage, TaskRef},
    util::tracing,
};
use core::{future::Future, pin::Pin};

use cordyceps::mpsc_queue::MpscQueue;

#[derive(Debug)]
#[cfg_attr(feature = "alloc", derive(Default))]
pub struct StaticScheduler(Core);

#[derive(Debug)]
struct Core {
    run_queue: MpscQueue<Header>,
    // woken: AtomicBool,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct Tick {
    pub polled: usize,
    pub completed: usize,
    pub has_remaining: bool,
}

pub trait Schedule: Sized + Clone {
    fn schedule(&self, task: TaskRef);
}

/// A stub [`Task`],
///
/// This represents a [`Task`] that will never actually be executed.
/// It is used exclusively for initializing a [`StaticScheduler`],
/// using the unsafe [`new_with_static_stub()`] method.
///
/// [`StaticScheduler`]: crate::scheduler::StaticScheduler
/// [`new_with_static_stub()`]: crate::scheduler::StaticScheduler::new_with_static_stub
#[repr(transparent)]
pub struct TaskStub {
    hdr: Header,
}

impl TaskStub {
    /// Create a new unique stub [`Task`].
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            hdr: Header::new_stub(),
        }
    }
}

// === impl StaticScheduler ===

impl StaticScheduler {
    /// How many tasks are polled per call to `StaticScheduler::tick`.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

    /// Create a StaticScheduler with a static "stub" task entity
    ///
    /// This is used for creating a StaticScheduler as a `static` variable.
    ///
    /// # Safety
    ///
    /// The "stub" provided must ONLY EVER be used for a single StaticScheduler.
    /// Re-using the stub for multiple schedulers may lead to undefined behavior.
    #[cfg(not(loom))]
    pub const unsafe fn new_with_static_stub(stub: &'static TaskStub) -> Self {
        StaticScheduler(Core::new_with_static_stub(&stub.hdr))
    }

    /// Spawn a pre-allocated task
    ///
    /// This method is used to spawn a task that requires some bespoke
    /// procedure of allocation, typically of a custom [`Storage`] implementor.
    ///
    /// [`Storage`]: crate::task::storage::Storage
    #[inline]
    pub fn spawn_allocated<F, STO>(&'static self, task: STO::StoredTask)
    where
        F: Future,
        STO: Storage<&'static Self, F>,
    {
        let tr = TaskRef::new_allocated::<&'static Self, F, STO>(task);
        self.schedule(tr);
    }

    pub fn tick(&'static self) -> Tick {
        self.0.tick_n(Self::DEFAULT_TICK_SIZE)
    }
}

impl Core {
    /// How many tasks are polled per call to `StaticScheduler::tick`.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    const DEFAULT_TICK_SIZE: usize = 256;

    #[cfg(not(loom))]
    const unsafe fn new_with_static_stub(stub: &'static Header) -> Self {
        Self {
            run_queue: MpscQueue::new_with_static_stub(stub),
        }
    }

    fn tick_n(&self, n: usize) -> Tick {
        let mut tick = Tick {
            polled: 0,
            completed: 0,
            has_remaining: true,
        };

        for task in self.run_queue.consume() {
            let span = tracing::debug_span!("poll", ?task);
            let _enter = span.enter();
            let poll = task.poll();
            if poll.is_ready() {
                tick.completed += 1;
            }
            tick.polled += 1;

            tracing::debug!(parent: span.id(), poll = ?poll, tick.polled, tick.completed);
            if tick.polled == n {
                return tick;
            }
        }

        // we drained the current run queue.
        tick.has_remaining = false;

        tick
    }
}

impl Schedule for &'static StaticScheduler {
    fn schedule(&self, task: TaskRef) {
        // self.woken.store(true, Ordering::Release);
        self.0.run_queue.enqueue(task);
    }
}

#[derive(Copy, Clone, Debug)]
struct Stub;

impl Schedule for Stub {
    fn schedule(&self, _: TaskRef) {
        unimplemented!("stub task should never be woken!")
    }
}

impl Future for Stub {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        unreachable!("the stub task should never be polled!")
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::test_util::{Chan, Yield};
    use super::*;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use mycelium_util::sync::Lazy;

    #[test]
    fn basically_works() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static IT_WORKED: AtomicBool = AtomicBool::new(false);

        SCHEDULER.spawn(async {
            Yield::once().await;
            IT_WORKED.store(true, Ordering::Release);
        });

        let tick = SCHEDULER.tick();

        assert!(IT_WORKED.load(Ordering::Acquire));
        assert_eq!(tick.completed, 1);
        assert!(!tick.has_remaining);
        assert_eq!(tick.polled, 2)
    }

    #[test]
    fn schedule_many() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        const TASKS: usize = 10;

        for _ in 0..TASKS {
            SCHEDULER.spawn(async {
                Yield::once().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            })
        }

        let tick = SCHEDULER.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(tick.polled, TASKS * 2);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }

    #[test]
    fn notify_future() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let chan = Chan::new(1);

        SCHEDULER.spawn({
            let chan = chan.clone();
            async move {
                chan.wait().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            }
        });

        SCHEDULER.spawn(async move {
            Yield::once().await;
            chan.notify();
        });

        dbg!(SCHEDULER.tick());

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn notify_external() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let chan = Chan::new(1);

        SCHEDULER.spawn({
            let chan = chan.clone();
            async move {
                chan.wait().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            }
        });

        std::thread::spawn(move || {
            chan.notify();
        });

        while dbg!(SCHEDULER.tick().completed) < 1 {
            std::thread::yield_now();
        }

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn many_yields() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        const TASKS: usize = 10;

        for i in 0..TASKS {
            SCHEDULER.spawn(async {
                Yield::new(i).await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            })
        }

        let tick = SCHEDULER.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }
}

// Additional types and capabilities only available with the "alloc"
// feature active
feature! {
    #![feature = "alloc"]

    use crate::{
        loom::sync::Arc,
        task::{BoxStorage, Task},
    };
    use alloc::boxed::Box;

    #[derive(Clone, Debug, Default)]
    pub struct Scheduler(Arc<Core>);

    // === impl Scheduler ===
    impl Scheduler {
        /// How many tasks are polled per call to `Scheduler::tick`.
        ///
        /// Chosen by fair dice roll, guaranteed to be random.
        pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

        pub fn new() -> Self {
            Self::default()
        }

        #[inline]
        pub fn spawn(&self, future: impl Future) {
            self.schedule(TaskRef::new(self.clone(), future));
        }

        #[inline]
        pub fn spawn_allocated<F>(&'static self, task: Box<Task<Self, F, BoxStorage>>)
        where
            F: Future,
        {
            let tr = TaskRef::new_allocated::<Self, F, BoxStorage>(task);
            self.schedule(tr);
        }

        pub fn tick(&self) -> Tick {
            self.0.tick_n(Self::DEFAULT_TICK_SIZE)
        }
    }

    impl Schedule for Scheduler {
        fn schedule(&self, task: TaskRef) {
            // self.0.woken.store(true, Ordering::Release);
            self.0.run_queue.enqueue(task);
        }
    }

    impl StaticScheduler {
        pub fn new() -> Self {
            Self::default()
        }

        #[inline]
        pub fn spawn(&'static self, future: impl Future) {
            self.schedule(TaskRef::new(self, future));
        }
    }

    impl Core {
        fn new() -> Self {
            let stub_task = TaskRef::new(Stub, Stub);
            Self {
                run_queue: MpscQueue::new_with_stub(test_dbg!(stub_task)),
            }
        }
    }

    impl Default for Core {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(all(test, loom))]
mod loom {
    use super::test_util::{Chan, Yield};
    use super::*;
    use crate::loom::{
        self,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread,
    };
    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    #[pin_project::pin_project]
    struct TrackFuture<F> {
        #[pin]
        inner: F,
        track: Arc<()>,
    }

    impl<F: Future> Future for TrackFuture<F> {
        type Output = TrackFuture<F::Output>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.project();
            this.inner.poll(cx).map(|inner| TrackFuture {
                inner,
                track: this.track.clone(),
            })
        }
    }

    fn track_future<F: Future>(inner: F) -> TrackFuture<F> {
        TrackFuture {
            inner,
            track: Arc::new(()),
        }
    }

    #[test]
    fn basically_works() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let it_worked = Arc::new(AtomicBool::new(false));

            scheduler.spawn({
                let it_worked = it_worked.clone();
                track_future(async move {
                    Yield::once().await;
                    it_worked.store(true, Ordering::Release);
                })
            });

            let tick = scheduler.tick();

            assert!(it_worked.load(Ordering::Acquire));
            assert_eq!(tick.completed, 1);
            assert!(!tick.has_remaining);
            assert_eq!(tick.polled, 2)
        })
    }

    #[test]
    fn notify_external() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let chan = Chan::new(1);
            let it_worked = Arc::new(AtomicBool::new(false));

            scheduler.spawn({
                let it_worked = it_worked.clone();
                let chan = chan.clone();
                track_future(async move {
                    chan.wait().await;
                    it_worked.store(true, Ordering::Release);
                })
            });

            thread::spawn(move || {
                chan.notify();
            });

            while scheduler.tick().completed < 1 {
                thread::yield_now();
            }

            assert!(it_worked.load(Ordering::Acquire));
        })
    }

    #[test]
    fn notify_future() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let chan = Chan::new(1);
            let it_worked = Arc::new(AtomicBool::new(false));

            scheduler.spawn({
                let it_worked = it_worked.clone();
                let chan = chan.clone();
                track_future(async move {
                    chan.wait().await;
                    it_worked.store(true, Ordering::Release);
                })
            });

            scheduler.spawn(async move {
                Yield::once().await;
                chan.notify();
            });

            test_dbg!(scheduler.tick());

            assert!(it_worked.load(Ordering::Acquire));
        })
    }

    #[test]
    fn schedule_many() {
        const TASKS: usize = 10;
        loom::model(|| {
            let scheduler = Scheduler::new();
            let completed = Arc::new(AtomicUsize::new(0));

            for _ in 0..TASKS {
                scheduler.spawn({
                    let completed = completed.clone();
                    track_future(async move {
                        Yield::once().await;
                        completed.fetch_add(1, Ordering::SeqCst);
                    })
                });
            }

            let tick = scheduler.tick();

            assert_eq!(tick.completed, TASKS);
            assert_eq!(tick.polled, TASKS * 2);
            assert_eq!(completed.load(Ordering::SeqCst), TASKS);
            assert!(!tick.has_remaining);
        })
    }

    #[test]
    #[ignore] // this hits what i *believe* is a loom bug: https://github.com/tokio-rs/loom/issues/260
    fn cross_thread_spawn() {
        const TASKS: usize = 10;
        loom::model(|| {
            let scheduler = Scheduler::new();
            let completed = Arc::new(AtomicUsize::new(0));
            let all_spawned = Arc::new(AtomicBool::new(false));
            loom::thread::spawn({
                let scheduler = scheduler.clone();
                let completed = completed.clone();
                let all_spawned = all_spawned.clone();
                move || {
                    for _ in 0..TASKS {
                        scheduler.spawn({
                            let completed = completed.clone();
                            track_future(async move {
                                Yield::once().await;
                                completed.fetch_add(1, Ordering::SeqCst);
                            })
                        });
                    }
                    all_spawned.store(true, Ordering::Release);
                }
            });

            let mut tick;
            loop {
                tick = scheduler.tick();
                if all_spawned.load(Ordering::Acquire) {
                    break;
                }
                loom::thread::yield_now();
            }

            assert_eq!(completed.load(Ordering::SeqCst), TASKS);
            assert!(!tick.has_remaining);
        })
    }
}

#[cfg(test)]
mod test_util {
    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    pub(crate) use crate::wait::cell::test_util::Chan;

    pub(crate) struct Yield {
        yields: usize,
    }

    impl Future for Yield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let yields = &mut self.as_mut().yields;
            if *yields == 0 {
                return Poll::Ready(());
            }
            *yields -= 1;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    impl Yield {
        pub(crate) fn once() -> Self {
            Self::new(1)
        }

        pub(crate) fn new(yields: usize) -> Self {
            Self { yields }
        }
    }
}
