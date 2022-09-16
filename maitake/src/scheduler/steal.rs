use super::*;
// use crate::loom::sync::atomic::{AtomicUsize, Ordering::*};
use cordyceps::mpsc_queue::MpscQueue;
use core::marker::PhantomData;

/// A work-stealing distributor queue.
pub struct Distributor<S> {
    queue: MpscQueue<Header>,
    // tasks: AtomicUsize,
    _scheduler_type: PhantomData<fn(S)>,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum TryStealError {
    Empty,
    Busy,
}

impl<S: Schedule> Distributor<S> {
    loom_const_fn! {
        /// # Safety
        ///
        /// The "stub" provided must ONLY EVER be used for a single
        /// `Distributor` instance. Re-using the stub for multiple distributors
        /// or schedulers may lead to undefined behavior.
        pub unsafe fn new_with_static_stub(stub: &'static TaskStub) -> Self {
            Self {
                queue: MpscQueue::new_with_static_stub(&stub.hdr),
                _scheduler_type: PhantomData,
            }
        }
    }

    pub fn spawn_allocated<STO, F>(&self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        STO: Storage<S, F>,
    {
        // self.tasks.fetch_add(1, Relaxed);
        let (task, join) = TaskRef::build_allocated::<S, F, STO>(TaskRef::NO_BUILDER, task);
        self.queue.enqueue(task);
        join
    }

    pub fn try_steal(&self, scheduler: &S) -> Result<usize, TryStealError> {
        let mut stolen = 0;
        let tasks = self.queue.try_consume().ok_or(TryStealError::Busy)?;
        for task in tasks {
            // TODO(eliza): probably handle cancelation by throwing out canceled
            // tasks here before binding them?
            unsafe {
                task.bind_scheduler(scheduler.clone());
            }
            scheduler.schedule(task);
            stolen += 1;
        }
        Ok(stolen)
    }
}

feature! {
    #![feature = "alloc"]

    use alloc::boxed::Box;
    use super::{BoxStorage, Task};

    impl<S: Schedule> Distributor<S> {
        pub fn new() -> Self {
            let stub_task = Box::new(Task::new_stub());
            let (stub_task, _) =
                TaskRef::new_allocated::<task::Stub, task::Stub, BoxStorage>(task::Stub, stub_task);
            Self {
                queue: MpscQueue::new_with_stub(stub_task),
                _scheduler_type: PhantomData,
            }

        }

        /// Spawns a new task with this builder's configured settings.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            let task = Box::new(Task::<S, _, BoxStorage>::new(future));
            self.spawn_allocated::<BoxStorage, _>(task)
        }
    }

    impl<S: Schedule> Default for Distributor<S> {
        fn default() -> Self {
            Self::new()
        }
    }
}
