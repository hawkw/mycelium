//! Schedulers for executing [tasks][task].
//!
//! In order to execute [asynchronous tasks][task], a system must have one or
//! more _schedulers_. A scheduler (also sometimes referred to as an
//! _executor_) is a component responsible for tracking which tasks have been
//! [woken], and [polling] them when they are ready to make progress.
//!
//! This module contains scheduler implementations for use with the [`maitake`
//! task system][task].
//!
//! # Using Schedulers
//!
//! This module provides two types which can be used as schedulers. These types
//! differ based on how the core data of the scheduler is shared with tasks
//! spawned on that scheduler:
//!
//! - [`Scheduler`]: a reference-counted single-core scheduler (requires the
//!       "alloc" feature). A [`Scheduler`] is internally implemented using an
//!       [`Arc`], and each task spawned on a [`Scheduler`] holds an `Arc` clone
//!       of the scheduler core.
//! - [`StaticScheduler`]: a single-core scheduler stored in a `static`
//!       variable. A [`StaticScheduler`] is referenced by tasks spawned on it
//!       as an `&'static StaticScheduler` reference. Therefore, it can be used
//!       without requiring `alloc`, and avoids atomic reference count
//!       increments when spawning tasks. However, in order to be used, a
//!       [`StaticScheduler`] *must* be stored in a `'static`, which can limit
//!       its usage in some cases.
//!
//! [woken]: crate::task::Waker::wake
//! [polling]: core::future::Future::poll
//! [task]: crate::task
#![warn(missing_docs, missing_debug_implementations)]
use crate::{
    loom::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::*},
    task::{self, Header, JoinHandle, Storage, TaskRef},
};
use core::{future::Future, ptr};

use cordyceps::mpsc_queue::MpscQueue;

#[cfg(any(feature = "tracing-01", feature = "tracing-02", test))]
use mycelium_util::fmt;

#[cfg(test)]
mod tests;

/// A statically-initialized scheduler implementation.
#[derive(Debug)]
#[cfg_attr(feature = "alloc", derive(Default))]
pub struct StaticScheduler(Core);

/// Metrics recorded during a scheduler tick.
///
/// This type is returned by the [`Scheduler::tick`] and
/// [`StaticScheduler::tick`] methods.
///
/// This type bundles together a number of values describing what occurred
/// during a scheduler tick, such as how many tasks were polled, how many of
/// those tasks completed, and how many new tasks were spawned since the last
/// tick.
///
/// Most of these values are primarily useful as performance and debugging
/// metrics. However, in some cases, they may also drive system behavior. For
/// example, the `has_remaining` field on this type indicates whether or not
/// more tasks are left in the scheduler's run queue after the tick. This can be
/// used to determine whether or not the system should continue ticking the
/// scheduler, or should perform other work before ticking again.
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

    /// The number of tasks that were woken from within their own `poll` calls
    /// during this tick.
    pub woken_internal: usize,
}

/// Trait implemented by schedulers.
///
/// This trait is implemented by the [`Scheduler`] and [`StaticScheduler`]
/// types. It is not intended to be publicly implemented by user-defined types,
/// but can be used to abstract over `static` and reference-counted schedulers.
pub trait Schedule: Sized + Clone {
    /// Schedule a task on this scheduler.
    ///
    /// This method is called by the task's [`Waker`] when a task is woken.
    ///
    /// [`Waker`]: core::task::Waker
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

/// A stub [`Task`].
///
/// This represents a [`Task`] that will never actually be executed.
/// It is used exclusively for initializing a [`StaticScheduler`],
/// using the unsafe [`new_with_static_stub()`] method.
///
/// [`StaticScheduler`]: crate::scheduler::StaticScheduler
/// [`new_with_static_stub()`]: crate::scheduler::StaticScheduler::new_with_static_stub
#[repr(transparent)]
#[cfg_attr(loom, allow(dead_code))]
#[derive(Debug)]
pub struct TaskStub {
    hdr: Header,
}

/// Safely constructs a new [`StaticScheduler`] instance in a `static`
/// initializer.
///
/// This macro is intended to be used as a `static` initializer:
///
/// ```rust
/// use maitake::scheduler;
///
/// // look ma, no `unsafe`!
/// static SCHEDULER: scheduler::StaticScheduler = scheduler::new_static!();
/// ```
///
/// Note that this macro is re-exported in the [`scheduler`] module as
/// [`scheduler::new_static!`], which feels somewhat more idiomatic than using
/// it at the crate-level; however, it is also available at the crate-level as
/// [`new_static_scheduler!`].
///
/// The [`StaticScheduler::new_with_static_stub`] constructor is unsafe to call,
/// because it requires that the [`TaskStub`] passed to the scheduler not be
/// used by other scheduler instances. This macro is a safe alternative to
/// manually initializing a [`StaticScheduler`] instance using
/// [`new_with_static_stub`], as it creates the stub task inside a scope,
/// ensuring that it cannot be referenceed by other [`StaticScheduler`]
/// instances.
///
/// This macro expands to the following code:
/// ```rust
/// # static SCHEDULER: maitake::scheduler::StaticScheduler =
/// {
///     static STUB_TASK: maitake::scheduler::TaskStub = maitake::scheduler::TaskStub::new();
///     unsafe {
///         // safety: `StaticScheduler::new_with_static_stub` is unsafe because
///         // the stub task must not be shared with any other `StaticScheduler`
///         // instance. because the `new_static` macro creates the stub task
///         // inside the scope of the static initializer, it is guaranteed that
///         // no other `StaticScheduler` instance can reference the `STUB_TASK`
///         // static, so this is always safe.
///         maitake::scheduler::StaticScheduler::new_with_static_stub(&STUB_TASK)
///     }
/// }
/// # ;
/// ```
///
/// [`new_with_static_stub`]: StaticScheduler::new_with_static_stub
/// [`scheduler`]: crate::scheduler
/// [`scheduler::new_static!`]: crate::scheduler::new_static!
/// [`new_static_scheduler!`]: crate::new_static_scheduler!
#[cfg(not(loom))]
#[macro_export]
macro_rules! new_static_scheduler {
    () => {{
        static STUB_TASK: $crate::scheduler::TaskStub = $crate::scheduler::TaskStub::new();
        unsafe {
            // safety: `StaticScheduler::new_with_static_stub` is unsafe because
            // the stub task must not be shared with any other `StaticScheduler`
            // instance. because the `new_static` macro creates the stub task
            // inside the scope of the static initializer, it is guaranteed that
            // no other `StaticScheduler` instance can reference the `STUB_TASK`
            // static, so this is always safe.
            $crate::scheduler::StaticScheduler::new_with_static_stub(&STUB_TASK)
        }
    }};
}

#[cfg(not(loom))]
pub use new_static_scheduler as new_static;

/// Core implementation of a scheduler, used by both the [`Scheduler`] and
/// [`StaticScheduler`] types.
///
/// Each scheduler instance (which must implement `Clone`) must own a
/// single instance of a `Core`, which is shared across clones of that scheduler.
#[derive(Debug)]
struct Core {
    /// The scheduler's run queue.
    ///
    /// This is an [atomic multi-producer, single-consumer queue][mpsc] of
    /// [`TaskRef`s]. When a task is [scheduled], it is pushed to this queue.
    /// When the scheduler polls tasks, they are dequeued from the queue
    /// and polled. If a task is woken during its poll, the scheduler
    /// will push it back to this queue. Otherwise, if the task doesn't
    /// self-wake, it will be pushed to the queue again if its [`Waker`]
    /// is woken.
    ///
    /// [mpsc]: cordyceps::MpscQueue
    /// [scheduled]: Schedule::schedule
    /// [`Waker`]: core::task::Waker
    run_queue: MpscQueue<Header>,

    /// The task currently being polled by this scheduler, if it is currently
    /// polling a task.
    ///
    /// If no task is currently being polled, this will be [`ptr::null_mut`].
    current_task: AtomicPtr<Header>,

    /// A counter of how many tasks were spawned since the last scheduler tick.
    spawned: AtomicUsize,

    /// A counter of how many tasks were woken from outside their own `poll`
    /// methods.
    woken_external: AtomicUsize,
}

// === impl TaskStub ===

impl TaskStub {
    loom_const_fn! {
        /// Create a new unique stub [`Task`].
        pub fn new() -> Self {
            Self {
                hdr: Header::new_static_stub(),
            }
        }
    }
}

// === impl StaticScheduler ===

impl StaticScheduler {
    /// How many tasks are polled per call to [`StaticScheduler::tick`].
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
    ///
    /// For a safe alternative, consider using the [`new_static!`] macro to
    /// initialize a `StaticScheduler` in a `static` variable.
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

    /// Tick this scheduler, polling up to [`DEFAULT_TICK_SIZE`] tasks from the
    /// scheduler's run queue.
    pub fn tick(&'static self) -> Tick {
        self.0.tick_n(Self::DEFAULT_TICK_SIZE)
    }
}

impl Core {
    /// How many tasks are polled per call to [`Core::tick`].
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
            let _span = debug_span!(
                "poll",
                task.addr = ?fmt::ptr(&task),
                task.tid = task.id().as_u64(),
            )
            .entered();
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

        /// Tick this scheduler, polling up to [`DEFAULT_TICK_SIZE`] tasks from the
        /// scheduler's run queue.
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
            let stub_task = Box::new(Task::new_stub());
            let (stub_task, _) = TaskRef::new_allocated::<task::Stub, task::Stub, BoxStorage>(stub_task);
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
