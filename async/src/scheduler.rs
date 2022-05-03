use crate::{
    loom::sync::atomic::{AtomicBool, Ordering},
    task::{Task, TaskRef},
};
use cordyceps::mpsc_queue::MpscQueue;
use core::{future::Future, pin::Pin, ptr::NonNull};

#[derive(Debug)]
pub struct StaticScheduler {
    run_queue: MpscQueue<TaskRef>,
    woken: AtomicBool,
}

#[derive(Debug)]
pub struct Tick {
    pub polled: usize,
    pub completed: usize,
    pub has_remaining: bool,
    _p: (),
}

pub(crate) trait Schedule: Sized + Clone {
    fn schedule(&self, task: NonNull<TaskRef>);
}

impl Schedule for &'static StaticScheduler {
    fn schedule(&self, task: NonNull<TaskRef>) {
        self.woken.store(true, Ordering::Release);
        self.run_queue.enqueue(task);
    }
}

impl StaticScheduler {
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
            woken: AtomicBool::new(false),
        }
    }

    pub fn spawn(&'static self, future: impl Future) {
        let task = Task::new(self, future);
        self.schedule(task.cast::<TaskRef>());
    }

    pub fn tick(&'static self) -> Tick {
        self.tick_n(Self::DEFAULT_TICK_SIZE)
    }

    fn tick_n(&'static self, n: usize) -> Tick {
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

#[derive(Copy, Clone)]
struct StubScheduler;
impl Schedule for StubScheduler {
    fn schedule(&self, _: NonNull<TaskRef>) {
        unimplemented!("stub task should never be woken!")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::Future,
        sync::atomic::AtomicUsize,
        task::{Context, Poll},
    };
    use mycelium_util::sync::Lazy;

    struct Yield {
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
        fn once() -> Self {
            Self::new(1)
        }

        fn new(yields: usize) -> Self {
            Self { yields }
        }
    }

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
