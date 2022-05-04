use crate::{
    loom::sync::atomic::{AtomicBool, Ordering},
    task::{Task, TaskRef},
};
use alloc::sync::Arc;
use cordyceps::mpsc_queue::MpscQueue;
use core::{future::Future, pin::Pin, ptr::NonNull, sync::Arc};

#[derive(Debug)]
pub struct Scheduler {
    run_queue: MpscQueue<TaskRef>,
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

impl Scheduler {
    /// How many tasks are polled per call to `StaticScheduler::tick`.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    pub const DEFAULT_TICK_SIZE: usize = 256;

    pub fn new() -> Self {
        let stub_task = Task::new(StubScheduler, async {
            unimplemented!("stub task should never be polled!")
        });
        Self {
            run_queue: MpscQueue::new_with_stub(stub_task),
            // woken: AtomicBool::new(false),
        }
    }

    pub fn spawn(&'static self, future: impl Future) {
        let task = Task::new(self, future);
        self.schedule(task.cast::<TaskRef>());
    }

    pub fn spawn_arc(self: Arc<Self>, future: impl Future) {
        let task = Task::new(self.clone(), future);
        self.schedule(task.cast::<TaskRef>());
    }

    pub fn tick(&self) -> Tick {
        self.tick_n(Self::DEFAULT_TICK_SIZE)
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

impl<F: Future> Spawn<F> for &'static Scheduler {
    #[inline]
    fn spawn(&self, future: F) {
        Scheduler::spawn(self, future)
    }
}

impl<F: Future> Spawn<F> for Arc<Scheduler> {
    #[inline]
    fn spawn(&self, future: F) {
        Scheduler::spawn_arc(self.clone(), future)
    }
}

impl Schedule for &'static Scheduler {
    fn schedule(&self, task: NonNull<TaskRef>) {
        // self.woken.store(true, Ordering::Release);
        self.run_queue.enqueue(task);
    }
}

impl Schedule for Arc<Scheduler> {
    fn schedule(&self, task: NonNull<TaskRef>) {
        // self.woken.store(true, Ordering::Release);
        self.run_queue.enqueue(task);
    }
}

#[derive(Copy, Clone)]
struct StubScheduler;
impl Schedule for StubScheduler {
    fn schedule(&self, _: NonNull<TaskRef>) {
        unimplemented!("stub task should never be woken!")
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
        static SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::new);
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
        static SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::new);
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
        static SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::new);
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
    use mycelium_util::sync::Lazy;

    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    fn with_leak_check<T: Future>(f: T) -> LeakCheck<T> {
        LeakCheck {
            inner: f,
            arc: Arc::new(()),
        }
    }

    #[pin_project::pin_project]
    struct LeakCheck<T> {
        #[pin]
        inner: T,
        // loom will report an error if an `Arc` is leaked
        arc: Arc<()>,
    }

    impl<T: Future> Future for LeakCheck<T> {
        type Output = LeakCheck<T::Output>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.project();
            this.inner.poll(cx).map(|inner| LeakCheck {
                inner,
                arc: this.arc.clone(),
            })
        }
    }

    #[test]
    fn basically_works() {
        static SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::new);
        loom::model(|| {
            let it_worked = Arc::new(AtomicBool::new(false));

            SCHEDULER.spawn({
                let it_worked = it_worked.clone();
                with_leak_check(async move {
                    Yield::once().await;
                    it_worked.store(true, Ordering::Release);
                })
            });

            let tick = SCHEDULER.tick();

            assert!(it_worked.load(Ordering::Acquire));
            assert_eq!(tick.completed, 1);
            assert!(!tick.has_remaining);
            assert_eq!(tick.polled, 2)
        })
    }

    #[test]
    fn schedule_many() {
        static SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::new);
        const TASKS: usize = 10;
        loom::model(|| {
            let completed = Arc::new(AtomicUsize::new(0));

            for _ in 0..TASKS {
                SCHEDULER.spawn({
                    let completed = completed.clone();
                    with_leak_check(async move {
                        Yield::once().await;
                        completed.fetch_add(1, Ordering::SeqCst);
                    })
                });
            }

            let tick = SCHEDULER.tick();

            assert_eq!(tick.completed, TASKS);
            assert_eq!(tick.polled, TASKS * 2);
            assert_eq!(completed.load(Ordering::SeqCst), TASKS);
            assert!(!tick.has_remaining);
        })
    }

    #[test]
    #[ignore] // this hits what i *believe* is a loom bug: https://github.com/tokio-rs/loom/issues/260
    fn cross_thread_spawn() {
        static SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::new);
        const TASKS: usize = 10;
        loom::model(|| {
            let completed = Arc::new(AtomicUsize::new(0));
            let all_spawned = Arc::new(AtomicBool::new(false));
            loom::thread::spawn({
                let completed = completed.clone();
                let all_spawned = all_spawned.clone();
                move || {
                    for _ in 0..TASKS {
                        SCHEDULER.spawn({
                            let completed = completed.clone();
                            with_leak_check(async move {
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
                tick = SCHEDULER.tick();
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
