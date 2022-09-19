use super::*;
// use crate::loom::sync::atomic::{AtomicUsize, Ordering::*};
use cordyceps::mpsc_queue::{self, MpscQueue};
use core::marker::PhantomData;

/// An injector queue for spawning tasks on multiple [`Scheduler`] instances.
pub struct Injector<S> {
    queue: MpscQueue<Header>,
    // tasks: AtomicUsize,
    _scheduler_type: PhantomData<fn(S)>,
}

/// A handle for stealing tasks from a [`Scheduler`]'s run queue, or an
/// [`Injector`] queue.
///
/// While this handle exists, no other worker can steal tasks from the queue.
pub struct Stealer<'worker, S> {
    queue: mpsc_queue::Consumer<'worker, Header>,
    _scheduler_type: PhantomData<fn(S)>,
}

impl<S: Schedule> Injector<S> {
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

    pub fn try_steal(&self) -> Option<Stealer<'_, S>> {
        let queue = self.queue.try_consume()?;
        Some(Stealer {
            queue,
            _scheduler_type: PhantomData,
        })
    }
}

impl<S: Schedule> Stealer<'_, S> {
    /// Steal one task from the targeted queue and spawn it on the provided
    /// `scheduler`.
    ///
    /// # Returns
    ///
    /// - `true` if a task was successfully stolen.
    /// - `false` if the targeted queue is empty.
    pub fn spawn_one(&self, scheduler: &S) -> bool {
        let task = match self.queue.dequeue() {
            Some(task) => task,
            None => return false,
        };

        // TODO(eliza): probably handle cancelation by throwing out canceled
        // tasks here before binding them?
        unsafe {
            task.bind_scheduler(scheduler.clone());
        }
        scheduler.schedule(task);
        true
    }

    /// Steal up to `max` tasks from the targeted queue and spawn them on the
    /// provided scheduler.
    ///
    /// # Returns
    ///
    /// The number of tasks stolen. This may be less than `max` if the targeted
    /// queue contained fewer tasks than `max`.
    pub fn spawn_n(&self, scheduler: &S, max: usize) -> usize {
        let mut stolen = 0;
        while stolen <= max && self.spawn_one(scheduler) {
            stolen += 1;
        }

        stolen
    }
}

impl StaticScheduler {
    /// Attempt to steal tasks from this scheduler's run queue.
    pub fn try_steal(&self) -> Option<Stealer<'_, &'static StaticScheduler>> {
        let queue = self.0.run_queue.try_consume()?;
        Some(Stealer {
            queue,
            _scheduler_type: PhantomData,
        })
    }
}

feature! {
    #![feature = "alloc"]

    use alloc::boxed::Box;
    use super::{BoxStorage, Task};

    impl<S: Schedule> Injector<S> {
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

    impl<S: Schedule> Default for Injector<S> {
        fn default() -> Self {
            Self::new()
        }
    }


    impl Scheduler {
        /// Attempt to steal tasks from this scheduler's run queue.
        pub fn try_steal(&self) -> Option<Stealer<'_, Scheduler>> {
            let queue = self.0.run_queue.try_consume()?;
            Some(Stealer {
                queue,
                _scheduler_type: PhantomData,
            })
        }
    }

}
