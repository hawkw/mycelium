use crate::{
    loom::sync::Arc,
    task::{self, Task, TaskRef},
};
use cordyceps::mpsc_queue::MpscQueue;
use core::{future::Future, pin::Pin, ptr::NonNull};
#[derive(Debug, Clone)]
pub struct Scheduler(Arc<Core>);

#[derive(Debug)]
pub struct StaticScheduler(Core);

#[derive(Debug)]
struct Core {
    run_queue: MpscQueue<TaskRef>,
    _stub_task: Arc<Task<Stub, Stub>>,
    // woken: AtomicBool,
}

#[derive(Debug)]
pub struct Tick {
    pub polled: usize,
    pub completed: usize,
    pub has_remaining: bool,
    _p: (),
}

/// A trait abstracting over spawning futures.
pub trait Spawn<F: Future> {
    /// Spawns `future` as a new task on this executor.
    fn spawn(&self, future: F);
}

pub(crate) trait Schedule: Sized + Clone {
    fn schedule(&self, task: NonNull<TaskRef>);
}

// === impl Scheduler ===

impl Scheduler {
    /// How many tasks are polled per call to `Scheduler::tick`.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

    pub fn new() -> Self {
        Self(Arc::new(Core::new()))
    }

    #[inline]
    pub fn spawn(&self, future: impl Future) {
        Core::spawn_arc(&self.0, future)
    }

    pub fn tick(&self) -> Tick {
        self.0.tick_n(Self::DEFAULT_TICK_SIZE)
    }
}

/// A trait abstracting over spawning futures.
impl<F: Future> Spawn<F> for Scheduler {
    /// Spawns `future` as a new task on this executor.
    fn spawn(&self, future: F) {
        Scheduler::spawn(self, future)
    }
}

// === impl StaticScheduler ===

impl StaticScheduler {
    /// How many tasks are polled per call to `StaticScheduler::tick`.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

    pub fn new() -> Self {
        Self(Core::new())
    }

    #[inline]
    pub fn spawn(&'static self, future: impl Future) {
        Core::spawn_static(&self.0, future)
    }

    pub fn tick(&'static self) -> Tick {
        self.0.tick_n(Self::DEFAULT_TICK_SIZE)
    }
}

/// A trait abstracting over spawning futures.
impl<F: Future> Spawn<F> for &'static StaticScheduler {
    /// Spawns `future` as a new task on this executor.
    fn spawn(&self, future: F) {
        StaticScheduler::spawn(self, future)
    }
}

impl Core {
    /// How many tasks are polled per call to `StaticScheduler::tick`.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    const DEFAULT_TICK_SIZE: usize = 256;

    fn new() -> Self {
        let stub_task = Arc::new(Task::new(Stub, Stub));
        let stub_ref = unsafe {
            let raw = Arc::into_raw(stub_task.clone());
            Arc::decrement_strong_count(raw);
            crate::util::non_null(raw as *mut Task<Stub, Stub>).cast()
        };
        Self {
            run_queue: MpscQueue::new_with_stub(stub_ref),
            _stub_task: stub_task, // woken: AtomicBool::new(false),
        }
    }

    fn spawn_static(&'static self, future: impl Future) {
        self.schedule(Task::new_ref(self, future));
    }

    fn spawn_arc(this: &Arc<Self>, future: impl Future) {
        this.schedule(Task::new_ref(this.clone(), future));
    }

    fn tick_n(&self, n: usize) -> Tick {
        let mut tick = Tick {
            polled: 0,
            completed: 0,
            has_remaining: true,
            _p: (),
        };

        for task in self.run_queue.consume() {
            if TaskRef::poll(task).is_ready() {
                tick.completed += 1;
            }
            tick.polled += 1;
            if tick.polled == n {
                return tick;
            }
        }

        // we drained the current run queue.
        tick.has_remaining = false;

        tick
    }
}

impl Schedule for &'static Core {
    fn schedule(&self, task: NonNull<TaskRef>) {
        // self.woken.store(true, Ordering::Release);
        self.run_queue.enqueue(task);
    }
}

impl Schedule for Arc<Core> {
    fn schedule(&self, task: NonNull<TaskRef>) {
        // self.woken.store(true, Ordering::Release);
        self.run_queue.enqueue(task);
    }
}

#[derive(Copy, Clone, Debug)]
struct Stub;

impl Schedule for Stub {
    fn schedule(&self, _: NonNull<TaskRef>) {
        unimplemented!("stub task should never be woken!")
    }
}

impl Future for Stub {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        unreachable!("the stub task should never be polled!")
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::test_util::Yield;
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

#[cfg(all(test, loom))]
mod loom {
    use super::test_util::Yield;
    use super::*;
    use crate::loom::{
        self,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };

    #[test]
    fn basically_works() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let it_worked = Arc::new(AtomicBool::new(false));

            scheduler.spawn({
                let it_worked = it_worked.clone();
                async move {
                    Yield::once().await;
                    it_worked.store(true, Ordering::Release);
                }
            });

            let tick = scheduler.tick();

            assert!(it_worked.load(Ordering::Acquire));
            assert_eq!(tick.completed, 1);
            assert!(!tick.has_remaining);
            assert_eq!(tick.polled, 2)
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
                    async move {
                        Yield::once().await;
                        completed.fetch_add(1, Ordering::SeqCst);
                    }
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
    // #[ignore] // this hits what i *believe* is a loom bug: https://github.com/tokio-rs/loom/issues/260
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
                            async move {
                                Yield::once().await;
                                completed.fetch_add(1, Ordering::SeqCst);
                            }
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
