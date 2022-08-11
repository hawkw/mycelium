use crate::{
    loom::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::*},
    task::{self, Header, JoinHandle, Storage, TaskRef},
};
use core::{future::Future, pin::Pin, ptr};

use cordyceps::mpsc_queue::MpscQueue;

#[derive(Debug)]
#[cfg_attr(feature = "alloc", derive(Default))]
pub struct StaticScheduler(Core);

#[derive(Debug)]
struct Core {
    run_queue: MpscQueue<Header>,
    current_task: AtomicPtr<Header>,

    /// A counter of how many tasks were spawned since the last scheduler tick.
    spawned: AtomicUsize,

    /// A counter of how many tasks were woken while not ticking the scheduler.
    woken_external: AtomicUsize,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct Tick {
    /// The total number of tasks polled on this scheduler tick.
    pub polled: usize,

    /// The number of polled tasks that *completed* on this scheduler tick.
    ///
    /// This should always be <= `self.polled`.
    pub completed: usize,

    /// `true` if the tick completed with any tasks remaining in the run queue.
    pub has_remaining: bool,

    /// The number of tasks that were spawned since the last tick.
    pub spawned: usize,

    /// The number of tasks that were woken from outside of their own `poll`
    /// calls since the last tick.
    pub woken_external: usize,

    /// The number of tasks that were woken from within their own poll calls
    /// during this tick.
    pub woken_internal: usize,
}

pub trait Schedule: Sized + Clone {
    fn schedule(&self, task: TaskRef);

    /// Returns a [`TaskRef`] referencing the task currently being polled by
    /// this scheduler, if a task is currently being polled.
    #[must_use]
    fn current_task(&self) -> Option<TaskRef>;

    /// Returns a new [task `Builder`] for configuring tasks prior to spawning
    /// them on this scheduler.
    ///
    /// [task `Builder`]: task::Builder
    #[must_use]
    fn build_task<'a>(&self) -> task::Builder<'a, Self> {
        task::Builder::new(self.clone())
    }
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
#[cfg_attr(loom, allow(dead_code))]
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
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    ///
    /// [`Storage`]: crate::task::Storage
    #[inline]
    #[track_caller]
    pub fn spawn_allocated<F, STO>(&'static self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
        STO: Storage<&'static Self, F>,
    {
        let (task, join) = TaskRef::new_allocated::<&'static Self, F, STO>(task);
        self.schedule(task);
        join
    }

    /// Returns a new [task `Builder`] for configuring tasks prior to spawning
    /// them on this scheduler.
    ///
    /// [task `Builder`]: task::Builder
    #[must_use]
    pub fn build_task<'a>(&'static self) -> task::Builder<'a, &'static Self> {
        task::Builder::new(self)
    }

    /// Returns a [`TaskRef`] referencing the task currently being polled by
    /// this scheduler, if a task is currently being polled.
    #[must_use]
    #[inline]
    pub fn current_task(&'static self) -> Option<TaskRef> {
        self.0.current_task()
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
            current_task: AtomicPtr::new(ptr::null_mut()),
            spawned: AtomicUsize::new(0),
            woken_external: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    fn current_task(&self) -> Option<TaskRef> {
        let ptr = self.current_task.load(Acquire);
        let ptr = ptr::NonNull::new(ptr)?;
        Some(TaskRef::clone_from_raw(ptr))
    }

    #[inline(always)]
    fn schedule(&self, task: TaskRef) {
        self.woken_external.fetch_add(1, Relaxed);
        self.run_queue.enqueue(task);
    }

    #[inline(always)]
    fn spawn_inner(&self, task: TaskRef) {
        self.spawned.fetch_add(1, Relaxed);
        self.run_queue.enqueue(task);
    }

    fn tick_n(&self, n: usize) -> Tick {
        use task::PollResult;

        let mut tick = Tick {
            polled: 0,
            completed: 0,
            spawned: 0,
            woken_external: 0,
            woken_internal: 0,
            has_remaining: false,
        };

        for task in self.run_queue.consume() {
            let _span = debug_span!("poll", ?task, task.tid = %task.id()).entered();
            // store the currently polled task in the `current_task` pointer.
            // using `TaskRef::as_ptr` is safe here, since we will clear the
            // `current_task` pointer before dropping the `TaskRef`.
            self.current_task.store(task.as_ptr().as_ptr(), Release);

            // poll the task
            let poll_result = task.poll();

            // clear the current task cell before potentially dropping the
            // `TaskRef`.
            self.current_task.store(ptr::null_mut(), Release);

            tick.polled += 1;
            match poll_result {
                PollResult::Ready | PollResult::ReadyJoined => tick.completed += 1,
                PollResult::PendingSchedule => {
                    self.run_queue.enqueue(task);
                    tick.woken_internal += 1;
                }
                PollResult::Pending => {}
            }

            debug!(poll = ?poll_result, tick.polled, tick.completed);
            if tick.polled == n {
                // we haven't drained the current run queue.
                tick.has_remaining = false;
                break;
            }
        }

        tick.spawned = self.spawned.swap(0, Relaxed);
        tick.woken_external = self.woken_external.swap(0, Relaxed);

        // log scheduler metrics.
        debug!(
            tick.polled,
            tick.completed,
            tick.spawned,
            tick.woken_external,
            tick.woken_internal,
            tick.has_remaining
        );

        tick
    }
}

impl Schedule for &'static StaticScheduler {
    fn schedule(&self, task: TaskRef) {
        self.0.schedule(task)
    }

    #[must_use]
    fn current_task(&self) -> Option<TaskRef> {
        self.0.current_task()
    }
}

#[derive(Copy, Clone, Debug)]
struct Stub;

impl Schedule for Stub {
    fn schedule(&self, _: TaskRef) {
        unimplemented!("stub task should never be woken!")
    }

    fn current_task(&self) -> Option<TaskRef> {
        None
    }
}

impl Future for Stub {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        unreachable!("the stub task should never be polled!")
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

        /// Returns a new [task `Builder`][`Builder`] for configuring tasks prior to spawning
        /// them on this scheduler.
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake::scheduler::Scheduler;
        ///
        /// let scheduler = Scheduler::new();
        /// scheduler.build_task().name("hello world").spawn(async {
        ///     // ...
        /// });
        ///
        /// scheduler.tick();
        /// ```
        ///
        /// Multiple tasks can be spawned using the same [`Builder`]:
        ///
        /// ```
        /// use maitake::scheduler::Scheduler;
        ///
        /// let scheduler = Scheduler::new();
        /// let builder = scheduler
        ///     .build_task()
        ///     .kind("my_cool_task");
        ///
        /// builder.spawn(async {
        ///     // ...
        /// });
        ///
        /// builder.spawn(async {
        ///     // ...
        /// });
        ///
        /// scheduler.tick();
        /// ```
        ///
        /// [`Builder`]: task::Builder
        #[must_use]
        #[inline]
        pub fn build_task<'a>(&self) -> task::Builder<'a, Self> {
            task::Builder::new(self.clone())
        }

        /// Spawn a task.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new(self.clone(), future);
            self.0.spawn_inner(task);
            join
        }


        /// Spawn a pre-allocated task
        ///
        /// This method is used to spawn a task that requires some bespoke
        /// procedure of allocation, typically of a custom [`Storage`] implementor.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// [`Storage`]: crate::task::Storage
        #[inline]
        #[track_caller]
        pub fn spawn_allocated<F>(&'static self, task: Box<Task<Self, F, BoxStorage>>) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new_allocated::<Self, F, BoxStorage>(task);
            self.0.spawn_inner(task);
            join
        }


        /// Returns a [`TaskRef`] referencing the task currently being polled by
        /// this scheduler, if a task is currently being polled.
        #[must_use]
        #[inline]
        pub fn current_task(&self) -> Option<TaskRef> {
            self.0.current_task()
        }

        pub fn tick(&self) -> Tick {
            self.0.tick_n(Self::DEFAULT_TICK_SIZE)
        }
    }

    impl Schedule for Scheduler {
        fn schedule(&self, task: TaskRef) {
            self.0.schedule(task)
        }

        #[must_use]
        fn current_task(&self) -> Option<TaskRef> {
            self.0.current_task()
        }
    }

    impl StaticScheduler {
        pub fn new() -> Self {
            Self::default()
        }

        /// Spawn a task.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&'static self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new(self, future);
            self.0.spawn_inner(task);
            join
        }
    }

    impl Core {
        fn new() -> Self {
            let (stub_task, _) = TaskRef::new(Stub, Stub);
            Self {
                run_queue: MpscQueue::new_with_stub(test_dbg!(stub_task)),
                current_task: AtomicPtr::new(ptr::null_mut()),
                spawned: AtomicUsize::new(0),
                woken_external: AtomicUsize::new(0),
            }
        }
    }

    impl Default for Core {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests;
