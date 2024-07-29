use super::*;
use crate::loom::sync::atomic::AtomicUsize;
use cordyceps::mpsc_queue::{self, MpscQueue};
use core::marker::PhantomData;
use mycelium_util::fmt;

/// An injector queue for spawning tasks on multiple [`Scheduler`] instances.
pub struct Injector<S> {
    /// The queue.
    queue: MpscQueue<Header>,

    /// The number of tasks in the queue.
    tasks: AtomicUsize,

    /// An `Injector` can only be used with [`Schedule`] implementations that
    /// are the same type, because the task allocation is sized based on the
    /// scheduler value.
    _scheduler_type: PhantomData<fn(S)>,
}

/// A handle for stealing tasks from a [`Scheduler`]'s run queue, or an
/// [`Injector`] queue.
///
/// While this handle exists, no other worker can steal tasks from the queue.
pub struct Stealer<'worker, S> {
    queue: mpsc_queue::Consumer<'worker, Header>,

    /// The initial task count in the target queue when this `Stealer` was created.
    snapshot: usize,

    /// A reference to the target queue's current task count. This is used to
    /// decrement the task count when stealing.
    tasks: &'worker AtomicUsize,

    /// The type of the [`Schedule`] implementation that tasks are being stolen
    /// from.
    ///
    /// This must be the same type as the scheduler that is stealing tasks, as
    /// the size of the scheduler value stored in the task must be the same.
    _scheduler_type: PhantomData<fn(S)>,
}

/// Errors returned by [`Injector::try_steal`], [`Scheduler::try_steal`], and
/// [`StaticScheduler::try_steal`].
#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum TryStealError {
    /// Tasks could not be stolen because the targeted queue already has a
    /// consumer.
    Busy,
    /// No tasks were available to steal.
    Empty,
}

impl<S: Schedule> Injector<S> {
    /// Returns a new injector queue.
    ///
    /// # Safety
    ///
    /// The "stub" provided must ONLY EVER be used for a single
    /// `Injector` instance. Re-using the stub for multiple distributors
    /// or schedulers may lead to undefined behavior.
    #[must_use]
    #[cfg(not(loom))]
    pub const unsafe fn new_with_static_stub(stub: &'static TaskStub) -> Self {
        Self {
            queue: MpscQueue::new_with_static_stub(&stub.hdr),
            tasks: AtomicUsize::new(0),
            _scheduler_type: PhantomData,
        }
    }

    /// Spawns a pre-allocated task on the injector queue.
    ///
    /// The spawned task will be executed by any
    /// [`Scheduler`]/[`StaticScheduler`] instance that runs tasks from this
    /// queue.
    ///
    /// This method is used to spawn a task that requires some bespoke
    /// procedure of allocation, typically of a custom [`Storage`] implementor.
    /// See the documentation for the [`Storage`] trait for more details on
    /// using custom task storage.
    ///
    /// When the "alloc" feature flag is available, tasks that do not require
    /// custom storage may be spawned using the [`Injector::spawn`] method,
    /// instead.
    ///
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    ///
    /// [`Storage`]: crate::task::Storage
    pub fn spawn_allocated<STO, F>(&self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        STO: Storage<S, F>,
    {
        self.tasks.fetch_add(1, Relaxed);
        let (task, join) = TaskRef::build_allocated::<S, F, STO>(TaskRef::NO_BUILDER, task);
        self.queue.enqueue(task);
        join
    }

    /// Attempt to take tasks from the injector queue.
    ///
    /// # Returns
    ///
    /// - `Ok(`[`Stealer`]`)) if tasks can be spawned from the injector
    ///   queue.
    /// - `Err`([`TryStealError::Empty`]`)` if there were no tasks in this
    ///   injector queue.
    /// - `Err`([`TryStealError::Busy`]`)` if another worker was already
    ///   taking tasks from this injector queue.
    pub fn try_steal(&self) -> Result<Stealer<'_, S>, TryStealError> {
        Stealer::try_new(&self.queue, &self.tasks)
    }
}

impl<S> fmt::Debug for Injector<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // determine if alt-mode is enabled *before* constructing the
        // `DebugStruct`, because that mutably borrows the formatter.
        let alt = f.alternate();

        let Self {
            queue,
            tasks,
            _scheduler_type,
        } = self;
        let mut debug = f.debug_struct("Injector");
        debug
            .field("queue", queue)
            .field("tasks", &tasks.load(Relaxed));

        // only include the kind of wordy type name field if alt-mode
        // (multi-line) formatting is enabled.
        if alt {
            debug.field(
                "scheduler",
                &format_args!("PhantomData<{}>", core::any::type_name::<S>()),
            );
        }

        debug.finish()
    }
}

// === impl Stealer ===

impl<'worker, S: Schedule> Stealer<'worker, S> {
    fn try_new(
        queue: &'worker MpscQueue<Header>,
        tasks: &'worker AtomicUsize,
    ) -> Result<Self, TryStealError> {
        let snapshot = tasks.load(Acquire);
        if snapshot == 0 {
            return Err(TryStealError::Empty);
        }

        let queue = queue.try_consume().ok_or(TryStealError::Busy)?;
        Ok(Self {
            queue,
            snapshot,
            tasks,
            _scheduler_type: PhantomData,
        })
    }

    /// Returns the number of tasks that were in the targeted queue when this
    /// `Stealer` was created.
    ///
    /// This number is *not* guaranteed to be greater than the *current* number
    /// of tasks returned by [`task_count`], as new tasks may be enqueued while
    /// stealing.
    ///
    /// [`task_count`]: Self::task_count
    pub fn initial_task_count(&self) -> usize {
        self.snapshot
    }

    /// Returns the number of tasks currently in the targeted queue.
    pub fn task_count(&self) -> usize {
        self.tasks.load(Acquire)
    }

    /// Steal one task from the targeted queue and spawn it on the provided
    /// `scheduler`.
    ///
    /// # Returns
    ///
    /// - `true` if a task was successfully stolen.
    /// - `false` if the targeted queue is empty.
    pub fn spawn_one(&self, scheduler: &S) -> bool {
        let Some(task) = self.queue.dequeue() else {
            return false;
        };
        test_trace!(?task, "stole");

        // decrement the target queue's task count
        self.tasks.fetch_sub(1, Release);

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

    /// Steal half of the tasks currently in the targeted queue and spawn them
    /// on the provided scheduler.
    ///
    /// This is a convenience method that is equivalent to the following:
    ///
    /// ```
    /// # fn docs() {
    /// # use maitake::scheduler::{StaticScheduler, Stealer};
    /// # let scheduler = unimplemented!();
    /// # let stealer: Stealer<'_, &'static StaticScheduler> = unimplemented!();
    /// stealer.spawn_n(&scheduler, stealer.initial_task_count() / 2);
    /// # }
    /// ```
    ///
    /// # Returns
    ///
    /// The number of tasks stolen.
    pub fn spawn_half(&self, scheduler: &S) -> usize {
        self.spawn_n(scheduler, self.initial_task_count() / 2)
    }
}

impl<S> fmt::Debug for Stealer<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // determine if alt-mode is enabled *before* constructing the
        // `DebugStruct`, because that mutably borrows the formatter.
        let alt = f.alternate();

        let Self {
            queue,
            snapshot,
            tasks,
            _scheduler_type,
        } = self;
        let mut debug = f.debug_struct("Stealer");
        debug
            .field("queue", queue)
            .field("snapshot", snapshot)
            .field("tasks", &tasks.load(Relaxed));

        // only include the kind of wordy type name field if alt-mode
        // (multi-line) formatting is enabled.
        if alt {
            debug.field(
                "scheduler",
                &format_args!("PhantomData<{}>", core::any::type_name::<S>()),
            );
        }

        debug.finish()
    }
}

// === impls on Scheduler types ===

impl StaticScheduler {
    /// Attempt to steal tasks from this scheduler's run queue.
    ///
    /// # Returns
    ///
    /// - `Ok(`[`Stealer`]`)) if tasks can be stolen from this scheduler's
    ///   queue.
    /// - `Err`([`TryStealError::Empty`]`)` if there were no tasks in this
    ///   scheduler's run queue.
    /// - `Err`([`TryStealError::Busy`]`)` if another worker was already
    ///   stealing from this scheduler's run queue.
    pub fn try_steal(&self) -> Result<Stealer<'_, &'static StaticScheduler>, TryStealError> {
        Stealer::try_new(&self.0.run_queue, &self.0.queued)
    }
}

feature! {
    #![feature = "alloc"]

    use alloc::boxed::Box;
    use super::{BoxStorage, Task};

    impl<S: Schedule> Injector<S> {
        /// Returns a new `Injector` queue with a dynamically heap-allocated
        /// [`TaskStub`].
        #[must_use]
        pub fn new() -> Self {
            let stub_task = Box::new(Task::new_stub());
            let (stub_task, _) =
                TaskRef::new_allocated::<task::Stub, task::Stub, BoxStorage>(task::Stub, stub_task);
            Self {
                queue: MpscQueue::new_with_stub(stub_task),
                tasks: AtomicUsize::new(0),
                _scheduler_type: PhantomData,
            }

        }

        /// Spawns a new task on the injector queue, to execute on any
        /// [`Scheduler`]/[`StaticScheduler`] instance that runs tasks from this
        /// queue.
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
        ///
        /// # Returns
        ///
        /// - `Ok(`[`Stealer`]`)) if tasks can be stolen from this scheduler's
        ///   queue.
        /// - `Err`([`TryStealError::Empty`]`)` if there were no tasks in this
        ///   scheduler's run queue.
        /// - `Err`([`TryStealError::Busy`]`)` if another worker was already
        ///   stealing from this scheduler's run queue.
        pub fn try_steal(&self) -> Result<Stealer<'_, Scheduler>, TryStealError> {
            Stealer::try_new(&self.0.run_queue, &self.0.queued)
        }
    }

}
