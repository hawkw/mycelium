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
//!       "alloc" [feature]). A [`Scheduler`] is internally implemented using an
//!       [`Arc`], and each task spawned on a [`Scheduler`] holds an `Arc` clone
//!       of the scheduler core.
//! - [`StaticScheduler`]: a single-core scheduler stored in a `static`
//!       variable. A [`StaticScheduler`] is referenced by tasks spawned on it
//!       as an `&'static StaticScheduler` reference. Therefore, it can be used
//!       without requiring `alloc`, and avoids atomic reference count
//!       increments when spawning tasks. However, in order to be used, a
//!       [`StaticScheduler`] *must* be stored in a `'static`, which can limit
//!       its usage in some cases.
//! - [`LocalScheduler`]: a reference-counted scheduler for `!`[`Send`] `Future`s
//!       (requires the "alloc" [feature]). This type is identical to the
//!       [`Scheduler`] type, except that it is capable of spawning `Future`s
//!       that do not implement [`Send`], and is itself not [`Send`] or [`Sync`]
//!       (it cannot be shared between CPU cores).
//! - [`LocalStaticScheduler`]: a [`StaticScheduler`] variant for `!`[`Send`]
//!       `Future`s.  This type is identical to the [`StaticScheduler`] type,
//!       except that it is capable of spawning `Future`s that do not implement
//!       [`Send`], and is itself not [`Send`] or [`Sync`] (it cannot be shared
//!       between CPU cores).
//!
//! The [`Schedule`] trait in this module is used by the [`Task`] type to
//! abstract over both types of scheduler that tasks may be spawned on.
//!
//! ## Spawning Tasks
//!
//! Once a scheduler has been constructed, tasks may be spawned on it using the
//! [`Scheduler::spawn`] or [`StaticScheduler::spawn`] methods. These methods
//! allocate a [new `Box` to store the spawned task](task::BoxStorage), and
//! therefore require the ["alloc" feature][feature].
//!
//! Alternatively, if [custom task storage](task::Storage) is in use, the
//! scheduler types also provide [`Scheduler::spawn_allocated`] and
//! [`StaticScheduler::spawn_allocated`] methods, which allow spawning a task
//! that has already been stored in a type implementing the [`task::Storage`]
//! trait. This can be used *without* the "alloc" feature flag, and is primarily
//! intended for use in systems where tasks are statically allocated, or where
//! an alternative allocator API (rather than `liballoc`) is in use.
//!
//! Finally, to configure the properties of a task prior to spawning it, both
//! scheduler types provide [`Scheduler::build_task`] and
//! [`StaticScheduler::build_task`] methods. These methods return a
//! [`task::Builder`] struct, which can be used to set properties of a task and
//! then spawn it on that scheduler.
//!
//! ## Executing Tasks
//!
//! In order to actually execute the tasks spawned on a scheduler, the scheduler
//! must be _driven_ by dequeueing tasks from its run queue and polling them.
//!
//! Because [`maitake` is a low-level async runtime "construction kit"][kit]
//! rather than a complete runtime implementation, the interface for driving a
//! scheduler is tick-based. A _tick_ refers to an iteration of a
//! scheduler's run loop, in which a set of tasks are dequeued from the
//! scheduler's run queue and polled. Calling the [`Scheduler::tick`] or
//! [`StaticScheduler::tick`] method on a  scheduler runs that scheduler for a
//! single tick, returning a [`Tick`] struct with data describing the events
//! that occurred during that tick.
//!
//! The scheduler API is tick-based, rather than providing methods that
//! continuously tick the scheduler until all tasks have completed, because
//! ticking a scheduler is often only one step of a system's run loop. A
//! scheduler is responsible for polling the tasks that have been woken, but it
//! does *not* wake tasks which are waiting for other runtime services, such as
//! timers and I/O resources.
//!
//! Typically, an iteration of a system's run loop consists of the following steps:
//!
//! - **Tick the scheduler**, executing any tasks that have been woken,
//! - **Tick a [timer][^1]**, to advance the system clock and wake any tasks waiting
//!   for time-based events,
//! - **Process wakeups from I/O resources**, such as hardware interrupts that
//!   occurred during the tick. The component responsible for this is often
//!   referred to as an [I/O reactor].
//! - Optionally, **spawn tasks from external sources**, such as work-stealing
//!   tasks from other schedulers, or receiving tasks from a remote system.
//!
//! The implementation of the timer and I/O runtime services in a bare-metal
//! system typically depend on details of the hardware platform in use.
//! Therefore, `maitake` does not provide a batteries-included runtime that
//! bundles together a scheduler, timer, and I/O reactor. Instead, the
//! lower-level tick-based scheduler interface allows running a `maitake`
//! scheduler as part of a run loop implementation that also drives other parts
//! of the runtime.
//!
//! A single call to [`Scheduler::tick`] will dequeue and poll up to
//! [`Scheduler::DEFAULT_TICK_SIZE`] tasks from the run queue, rather than
//! looping until all tasks in the queue have been dequeued.
//!
//! ## Examples
//!
//! A simple implementation of a system's run loop might look like this:
//!
//! ```rust
//! use maitake::scheduler::Scheduler;
//!
//! /// Process any time-based events that have occurred since this function
//! /// was last called.
//! fn process_timeouts() {
//!     // this might tick a `maitake::time::Timer` or run some other form of
//!     // time driver implementation.
//! }
//!
//!
//! /// Process any I/O events that have occurred since this function
//! /// was last called.
//! fn process_io_events() {
//!     // this function would handle dispatching any I/O interrupts that
//!     // occurred during the tick to tasks that are waiting for those I/O
//!     // events.
//! }
//!
//! /// Put the system into a low-power state until a hardware interrupt
//! /// occurs.
//! fn wait_for_interrupts() {
//!     // the implementation of this function would, of course, depend on the
//!     // hardware platform in use...
//! }
//!
//! /// The system's main run loop.
//! fn run_loop() {
//!     let scheduler = Scheduler::new();
//!
//!     loop {
//!         // process time-based events
//!         process_timeouts();
//!
//!         // process I/O events
//!         process_io_events();
//!
//!         // tick the scheduler, running any tasks woken by processing time
//!         // and I/O events, as well as tasks woken by other tasks during the
//!         // tick.
//!         let tick = scheduler.tick();
//!
//!         if !tick.has_remaining {
//!             // if the scheduler's run queue is empty, wait for an interrupt
//!             // to occur before ticking the scheduler again.
//!             wait_for_interrupts();
//!         }
//!     }
//! }
//! ```
//!
//! [^1]: The [`maitake::time`](crate::time) module provides one
//!     [`Timer`](crate::time::Timer) implementation, but other timers could be
//!     used as well.
//!
//! # Scheduling in Multi-Core Systems
//!
//! WIP ELIZA WRITE THIS
//!
//! [woken]: task::Waker::wake
//! [polling]: core::future::Future::poll
//! [task]: crate::task
//! [feature]: crate#features
//! [kit]: crate#maitake-is-not-a-complete-asynchronous-runtime
//! [timer]: crate::time
//! [I/O reactor]: https://en.wikipedia.org/wiki/Reactor_pattern
#![warn(missing_docs, missing_debug_implementations)]
use crate::{
    loom::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::*},
    task::{self, Header, JoinHandle, Storage, TaskRef},
};
use core::{future::Future, marker::PhantomData, ptr};

use cordyceps::mpsc_queue::{MpscQueue, TryDequeueError};

#[cfg(any(feature = "tracing-01", feature = "tracing-02", test))]
use mycelium_util::fmt;

mod steal;
#[cfg(test)]
mod tests;

pub use self::steal::*;

/// A statically-initialized scheduler implementation.
///
/// This type stores the core of the scheduler behind an `&'static` reference,
/// which is passed into each task spawned on the scheduler. This means that, in
/// order to spawn tasks, the `StaticScheduler` *must* be stored in a `static`
/// variable.
///
/// The use of a `&'static` reference allows `StaticScheduler`s to be used
/// without `liballoc`. In addition, spawning and deallocating tasks is slightly
/// cheaper than when using the reference-counted [`Scheduler`] type, because an
/// atomic reference count increment/decrement is not required.
///
/// # Usage
///
/// A `StaticScheduler` may be created one of two ways, depending on how the
/// [stub task] used by the MPSC queue algorithm is created: either with a
/// statically-allocated stub task by using the
/// [`StaticScheduler::new_with_static_stub`] function, or with a heap-allocated
/// stub task using [`StaticScheduler::new`] or [`StaticScheduler::default`]
/// (which require the ["alloc" feature][features]).
///
/// The [`new_with_static_stub`] function is a `const fn`, which allows a
/// `StaticScheduler` to be constructed directly in a `static` initializer.
/// However, it requires the [`TaskStub`] to be constructed manually by the
/// caller and passed in to initialize the scheduler. Furthermore, this function
/// is `unsafe` to call, as it requires that the provided [`TaskStub`] *not* be
/// used by any other `StaticScheduler` instance, which the function cannot
/// ensure.
///
/// For example:
///
/// ```rust
/// use maitake::scheduler::{self, StaticScheduler};
///
/// static SCHEDULER: StaticScheduler = {
///     // create a new static stub task *inside* the initializer for the
///     // `StaticScheduler`. since the stub task static cannot be referenced
///     // outside of this block, we can ensure that it is not used by any
///     // other calls to `StaticScheduler::new_with_static_stub`.
///     static STUB_TASK: scheduler::TaskStub = scheduler::TaskStub::new();
///
///     // now, create the scheduler itself:
///     unsafe {
///         // safety: creating the stub task inside the block used as an
///         // initializer expression for the scheduler static ensures that
///         // the stub task is not used by any other scheduler instance.
///         StaticScheduler::new_with_static_stub(&STUB_TASK)
///     }
/// };
///
/// // now, we can use the scheduler to spawn tasks:
/// SCHEDULER.spawn(async { /* ... */ });
/// ```
///
/// The [`scheduler::new_static!`] macro abstracts over the above code, allowing
/// a `static StaticScheduler` to be initialized without requiring the caller to
/// manually write `unsafe` code:
///
/// ```rust
/// use maitake::scheduler::{self, StaticScheduler};
///
/// // this macro expands to code identical to the previous example.
/// static SCHEDULER: StaticScheduler = scheduler::new_static!();
///
/// // now, we can use the scheduler to spawn tasks:
/// SCHEDULER.spawn(async { /* ... */ });
/// ```
///
/// Alternatively, the [`new`] and [`default`] constructors can be used to
/// create a new `StaticScheduler` with a heap-allocated stub task. This does
/// not require the user to manually create a stub task and ensure that it is
/// not used by any other `StaticScheduler` instances. However, these
/// constructors are not `const fn`s and require the ["alloc" feature][features]
/// to be enabled.
///
/// Because [`StaticScheduler::new`] and [`StaticScheduler::default`] are not
/// `const fn`s, but the scheduler must still be stored in a `static` to be
/// used, some form of lazy initialization of the `StaticScheduler` is necessary:
///
/// ```rust
/// use maitake::scheduler::StaticScheduler;
/// use mycelium_util::sync::Lazy;
///
/// static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
///
/// // now, we can use the scheduler to spawn tasks:
/// SCHEDULER.spawn(async { /* ... */ });
/// ```
///
/// Although the scheduler itself is no longer constructed in a `const fn`
/// static initializer in this case, storing it in a `static` rather than an
/// [`Arc`] still provides a minor performance benefit, as it avoids atomic
/// reference counting when spawning tasks.
///
/// [stub task]: TaskStub
/// [features]: crate#features
/// [`new_with_static_stub`]: Self::new_with_static_stub
/// [`scheduler::new_static!`]: new_static!
/// [`new`]: Self::new
/// [`default`]: Self::default
#[derive(Debug)]
#[cfg_attr(feature = "alloc", derive(Default))]
pub struct StaticScheduler(Core);

/// A statically-initialized scheduler for `!`[`Send`] tasks.
///
/// This type is identical to the [`StaticScheduler`] type, except that it is
/// capable of scheduling [`Future`]s that do not implement [`Send`]. Because
/// this scheduler's futures cannot be moved across threads[^1], the scheduler
/// itself is also `!Send` and `!Sync`, as ticking it from another thread would
/// cause its tasks to be polled from that thread, violating the [`Send`] and
/// [`Sync`] contracts.
///
/// [^1]: Or CPU cores, in bare-metal systems.
#[derive(Debug)]
#[cfg_attr(feature = "alloc", derive(Default))]
pub struct LocalStaticScheduler {
    core: Core,
    _not_send: PhantomData<*mut ()>,
}

/// A handle to a [`LocalStaticScheduler`] that implements [`Send`].
///
/// The [`LocalScheduler`] and [`LocalStaticScheduler`] types are capable of
/// spawning futures which do not implement [`Send`]. Because of this, those
/// scheduler types themselves are also `!Send` and `!Sync`, as as ticking them
/// from another thread would  cause its tasks to be polled from that thread,
/// violating the [`Send`] and [`Sync`] contracts.
///
/// However, tasks which *are* [`Send`] may still be spawned on a `!Send`
/// scheduler, alongside `!Send` tasks. Because the scheduler types are `!Sync`,
/// other threads may not reference them in order to spawn remote tasks on those
/// schedulers. This type is a handle to a `!Sync` scheduler which *can* be sent
/// across thread boundaries, as it does not have the capacity to poll tasks or
/// reference the current task.
///
/// This type is returned by [`LocalStaticScheduler::spawner`].
#[derive(Debug)]
pub struct LocalStaticSpawner(&'static LocalStaticScheduler);

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
pub trait Schedule: Sized + Clone + 'static {
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
    /// [`TaskRef`]s. When a task is [scheduled], it is pushed to this queue.
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

    /// A counter of how many tasks are in the scheduler's run queue.
    queued: AtomicUsize,

    /// A counter of how many tasks were woken from outside their own `poll`
    /// methods.
    woken: AtomicUsize,
}

// === impl TaskStub ===

impl TaskStub {
    loom_const_fn! {
        /// Create a new unique stub [`Task`].
        // Thee whole point of this thing is to be const-initialized, so a
        // non-const-fn `Default` impl is basically useless.
        #[allow(clippy::new_without_default)]
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
    /// See the documentation for the [`Storage`] trait for more details on
    /// using custom task storage.
    ///
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    ///
    /// When tasks are spawned on a scheduler, the scheduler must be
    /// [ticked](Self::tick) in order to drive those tasks to completion.
    /// See the [module-level documentation][run-loops] for more information
    /// on implementing a system's run loop.
    ///
    /// [`Storage`]: crate::task::Storage
    /// [run-loops]: crate::scheduler#executing-tasks
    #[inline]
    #[track_caller]
    pub fn spawn_allocated<F, STO>(&'static self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        STO: Storage<&'static Self, F>,
    {
        let (task, join) = TaskRef::new_allocated::<&'static Self, F, STO>(self, task);
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
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`TaskRef`]`)` referencing the currently-polling task, if a
    ///   task is currently being polled (i.e., the scheduler is
    ///   [ticking](Self::tick) and the queue of scheduled tasks is non-empty).
    ///
    /// - [`None`] if the scheduler is not currently being polled (i.e., the
    ///   scheduler is not ticking or its run queue is empty and all polls have
    ///   completed).
    #[must_use]
    #[inline]
    pub fn current_task(&'static self) -> Option<TaskRef> {
        self.0.current_task()
    }

    /// Tick this scheduler, polling up to [`Self::DEFAULT_TICK_SIZE`] tasks
    /// from the scheduler's run queue.
    ///
    /// Only a single CPU core/thread may tick a given scheduler at a time. If
    /// another call to `tick` is in progress on a different core, this method
    /// will immediately return.
    ///
    /// See [the module-level documentation][run-loops] for more information on
    /// using this function to implement a system's run loop.
    ///
    /// # Returns
    ///
    /// A [`Tick`] struct with data describing what occurred during the
    /// scheduler tick.
    ///
    /// [run-loops]: crate::scheduler#executing-tasks
    pub fn tick(&'static self) -> Tick {
        self.0.tick_n(Self::DEFAULT_TICK_SIZE)
    }
}

impl Schedule for &'static StaticScheduler {
    fn schedule(&self, task: TaskRef) {
        self.0.wake(task)
    }

    #[must_use]
    fn current_task(&self) -> Option<TaskRef> {
        self.0.current_task()
    }
}

// === impl LocalStaticScheduler ===

impl LocalStaticScheduler {
    /// How many tasks are polled per call to [`LocalStaticScheduler::tick`].
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

    /// Create a `LocalStaticScheduler` with a static "stub" task entity
    ///
    /// This is used for creating a `LocalStaticScheduler` as a `static` variable.
    ///
    /// # Safety
    ///
    /// The "stub" provided must ONLY EVER be used for a single `LocalStaticScheduler`.
    /// Re-using the stub for multiple schedulers may lead to undefined behavior.
    ///
    /// For a safe alternative, consider using the [`new_static!`] macro to
    /// initialize a `LocalStaticScheduler` in a `static` variable.
    #[cfg(not(loom))]
    pub const unsafe fn new_with_static_stub(stub: &'static TaskStub) -> Self {
        LocalStaticScheduler {
            core: Core::new_with_static_stub(&stub.hdr),
            _not_send: PhantomData,
        }
    }

    /// Spawn a pre-allocated, ![`Send`] task.
    ///
    /// Unlike [`StaticScheduler::spawn_allocated`], this method is capable of
    /// spawning [`Future`]s which do not implement [`Send`].
    ///
    /// This method is used to spawn a task that requires some bespoke
    /// procedure of allocation, typically of a custom [`Storage`] implementor.
    /// See the documentation for the [`Storage`] trait for more details on
    /// using custom task storage.
    ///
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    ///
    /// When tasks are spawned on a scheduler, the scheduler must be
    /// [ticked](Self::tick) in order to drive those tasks to completion.
    /// See the [module-level documentation][run-loops] for more information
    /// on implementing a system's run loop.
    ///
    /// [`Storage`]: crate::task::Storage
    /// [run-loops]: crate::scheduler#executing-tasks
    #[inline]
    #[track_caller]
    pub fn spawn_allocated<F, STO>(&'static self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
        STO: Storage<&'static Self, F>,
    {
        let (task, join) = TaskRef::new_allocated::<&'static Self, F, STO>(self, task);
        self.schedule(task);
        join
    }

    /// Returns a new [task `Builder`] for configuring tasks prior to spawning
    /// them on this scheduler.
    ///
    /// To spawn `!`[`Send`] tasks using a [`Builder`](task::Builder), use the
    /// [`Builder::spawn_local`](task::Builder::spawn_local) method.
    ///
    /// [task `Builder`]: task::Builder
    #[must_use]
    pub fn build_task<'a>(&'static self) -> task::Builder<'a, &'static Self> {
        task::Builder::new(self)
    }

    /// Returns a [`TaskRef`] referencing the task currently being polled by
    /// this scheduler, if a task is currently being polled.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`TaskRef`]`)` referencing the currently-polling task, if a
    ///   task is currently being polled (i.e., the scheduler is
    ///   [ticking](Self::tick) and the queue of scheduled tasks is non-empty).
    ///
    /// - [`None`] if the scheduler is not currently being polled (i.e., the
    ///   scheduler is not ticking or its run queue is empty and all polls have
    ///   completed).
    #[must_use]
    #[inline]
    pub fn current_task(&'static self) -> Option<TaskRef> {
        self.core.current_task()
    }

    /// Tick this scheduler, polling up to [`Self::DEFAULT_TICK_SIZE`] tasks
    /// from the scheduler's run queue.
    ///
    /// Only a single CPU core/thread may tick a given scheduler at a time. If
    /// another call to `tick` is in progress on a different core, this method
    /// will immediately return.
    ///
    /// See [the module-level documentation][run-loops] for more information on
    /// using this function to implement a system's run loop.
    ///
    /// # Returns
    ///
    /// A [`Tick`] struct with data describing what occurred during the
    /// scheduler tick.
    ///
    /// [run-loops]: crate::scheduler#executing-tasks
    pub fn tick(&'static self) -> Tick {
        self.core.tick_n(Self::DEFAULT_TICK_SIZE)
    }

    /// Returns a new [`LocalStaticSpawner`] that can be used by other threads to
    /// spawn [`Send`] tasks on this scheduler.
    #[must_use = "the returned `LocalStaticSpawner` does nothing unless used to spawn tasks"]
    pub fn spawner(&'static self) -> LocalStaticSpawner {
        LocalStaticSpawner(self)
    }
}

impl Schedule for &'static LocalStaticScheduler {
    fn schedule(&self, task: TaskRef) {
        self.core.wake(task)
    }

    #[must_use]
    fn current_task(&self) -> Option<TaskRef> {
        self.core.current_task()
    }
}

// === impl LocalStaticSpawner ===

impl LocalStaticSpawner {
    /// Spawn a pre-allocated task on the [`LocalStaticScheduler`] this spawner
    /// references.
    ///
    /// Unlike [`LocalScheduler::spawn_allocated`] and
    /// [`LocalStaticScheduler::spawn_allocated`], this method requires that the
    /// spawned `Future` implement [`Send`], as the `LocalSpawner` type is [`Send`]
    /// and [`Sync`], and therefore allows tasks to be spawned on a local
    /// scheduler from other threads.
    ///
    /// This method is used to spawn a task that requires some bespoke
    /// procedure of allocation, typically of a custom [`Storage`] implementor.
    /// See the documentation for the [`Storage`] trait for more details on
    /// using custom task storage.
    ///
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    ///
    /// When tasks are spawned on a scheduler, the scheduler must be
    /// [ticked](LocalStaticScheduler::tick) in order to drive those tasks to completion.
    /// See the [module-level documentation][run-loops] for more information
    /// on implementing a system's run loop.
    ///
    /// [`Storage`]: crate::task::Storage
    /// [run-loops]: crate::scheduler#executing-tasks
    #[inline]
    #[track_caller]
    pub fn spawn_allocated<F, STO>(&self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        STO: Storage<&'static LocalStaticScheduler, F>,
    {
        self.0.spawn_allocated::<F, STO>(task)
    }
}

/// # Safety
///
/// A `LocalStaticSpawner` cannot be used to access any `!Send` tasks on the
/// local scheduler it references. It can only push tasks to that scheduler's
/// run queue, which *is* thread safe.
unsafe impl Send for LocalStaticSpawner {}
unsafe impl Sync for LocalStaticSpawner {}

// === impl Core ===

impl Core {
    /// How many tasks are polled per scheduler tick.
    ///
    /// Chosen by fair dice roll, guaranteed to be random.
    const DEFAULT_TICK_SIZE: usize = 256;

    #[cfg(not(loom))]
    const unsafe fn new_with_static_stub(stub: &'static Header) -> Self {
        Self {
            run_queue: MpscQueue::new_with_static_stub(stub),
            current_task: AtomicPtr::new(ptr::null_mut()),
            queued: AtomicUsize::new(0),
            spawned: AtomicUsize::new(0),
            woken: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    fn current_task(&self) -> Option<TaskRef> {
        let ptr = self.current_task.load(Acquire);
        let ptr = ptr::NonNull::new(ptr)?;
        Some(TaskRef::clone_from_raw(ptr))
    }

    /// Wake `task`, adding it to the scheduler's run queue.
    #[inline(always)]
    fn wake(&self, task: TaskRef) {
        self.woken.fetch_add(1, Relaxed);
        self.schedule(task)
    }

    /// Schedule `task` for execution, adding it to this scheduler's run queue.
    #[inline]
    fn schedule(&self, task: TaskRef) {
        self.queued.fetch_add(1, Relaxed);
        self.run_queue.enqueue(task);
    }

    #[inline(always)]
    fn spawn_inner(&self, task: TaskRef) {
        // ensure the woken bit is set when spawning so the task won't be queued twice.
        task.set_woken();
        self.spawned.fetch_add(1, Relaxed);
        self.schedule(task);
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

        while tick.polled < n {
            let task = match self.run_queue.try_dequeue() {
                Ok(task) => task,
                // If inconsistent, just try again.
                Err(TryDequeueError::Inconsistent) => {
                    core::hint::spin_loop();
                    continue;
                }
                // Queue is empty or busy (in use by something else), bail out.
                Err(TryDequeueError::Busy | TryDequeueError::Empty) => {
                    break;
                }
            };

            self.queued.fetch_sub(1, Relaxed);
            let _span = trace_span!(
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
                    self.schedule(task);
                    tick.woken_internal += 1;
                }
                PollResult::Pending => {}
            }

            trace!(poll = ?poll_result, tick.polled, tick.completed);
        }

        tick.spawned = self.spawned.swap(0, Relaxed);
        tick.woken_external = self.woken.swap(0, Relaxed);

        // are there still tasks in the queue? if so, we have more tasks to poll.
        if test_dbg!(self.queued.load(Relaxed)) > 0 {
            tick.has_remaining = true;
        }

        if tick.polled > 0 {
            // log scheduler metrics.
            debug!(
                tick.polled,
                tick.completed,
                tick.spawned,
                tick.woken = tick.woken(),
                tick.woken.external = tick.woken_external,
                tick.woken.internal = tick.woken_internal,
                tick.has_remaining
            );
        }

        tick
    }
}

impl Tick {
    /// Returns the total number of tasks woken since the last poll.
    pub fn woken(&self) -> usize {
        self.woken_external + self.woken_internal
    }
}

// Additional types and capabilities only available with the "alloc"
// feature active
feature! {
    #![feature = "alloc"]

    use crate::{
        loom::sync::{Arc},
        task::{BoxStorage, Task},
    };
    use alloc::{sync::Weak, boxed::Box};

    /// An atomically reference-counted single-core scheduler implementation.
    ///
    /// This type stores the core of the scheduler inside an [`Arc`], which is
    /// cloned by each task spawned on the scheduler. The use of [`Arc`] allows
    /// schedulers to be created and dropped dynamically at runtime. This is in
    /// contrast to the [`StaticScheduler`] type, which must be stored in a
    /// `static` variable for the entire lifetime of the program.
    ///
    /// Due to the use of [`Arc`], this type requires [the "alloc" feature
    /// flag][features] to be enabled.
    ///
    /// [features]: crate#features
    #[derive(Clone, Debug, Default)]
    pub struct Scheduler(Arc<Core>);

    /// A reference-counted scheduler for `!`[`Send`] tasks.
    ///
    /// This type is identical to the [`LocalScheduler`] type, except that it is
    /// capable of scheduling [`Future`]s that do not implement [`Send`]. Because
    /// this scheduler's futures cannot be moved across threads[^1], the scheduler
    /// itself is also `!Send` and `!Sync`, as ticking it from multiple threads would
    /// move ownership of a `!Send` future.
    ///
    /// This type stores the core of the scheduler inside an [`Arc`], which is
    /// cloned by each task spawned on the scheduler. The use of [`Arc`] allows
    /// schedulers to be created and dropped dynamically at runtime. This is in
    /// contrast to the [`StaticScheduler`] type, which must be stored in a
    /// `static` variable for the entire lifetime of the program.
    ///
    /// Due to the use of [`Arc`], this type requires [the "alloc" feature
    /// flag][features] to be enabled.
    ///
    /// [features]: crate#features
    /// [^1]: Or CPU cores, in bare-metal systems.
    #[derive(Clone, Debug, Default)]
    pub struct LocalScheduler {
        core: Arc<Core>,
        _not_send: PhantomData<*mut ()>,
    }

    /// A handle to a [`LocalScheduler`] that implements [`Send`].
    ///
    /// The [`LocalScheduler`] and [`LocalStaticScheduler`] types are capable of
    /// spawning futures which do not implement [`Send`]. Because of this, those
    /// scheduler types themselves are also `!Send` and `!Sync`, as as ticking them
    /// from another thread would  cause its tasks to be polled from that thread,
    /// violating the [`Send`] and [`Sync`] contracts.
    ///
    /// However, tasks which *are* [`Send`] may still be spawned on a `!Send`
    /// scheduler, alongside `!Send` tasks. Because the scheduler types are `!Sync`,
    /// other threads may not reference them in order to spawn remote tasks on those
    /// schedulers. This type is a handle to a `!Sync` scheduler which *can* be sent
    /// across thread boundaries, as it does not have the capacity to poll tasks or
    /// reference the current task.
    ///
    /// This type owns a [`Weak`] reference to the scheduler. If the
    /// `LocalScheduler` is dropped, any attempts to spawn a task using this
    /// handle will return a [`JoinHandle`] that fails with a "scheduler shut
    /// down" error.
    ///
    /// This type is returned by [`LocalScheduler::spawner`].
    #[derive(Clone, Debug)]
    pub struct LocalSpawner(Weak<Core>);

    // === impl Scheduler ===

    impl Scheduler {
        /// How many tasks are polled per call to `Scheduler::tick`.
        ///
        /// Chosen by fair dice roll, guaranteed to be random.
        pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

        /// Returns a new `Scheduler`.
        #[must_use]
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

        /// Spawn a [task].
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](Self::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// # Examples
        ///
        /// Spawning a task and awaiting its output:
        ///
        /// ```
        /// use maitake::scheduler::Scheduler;
        ///
        /// let scheduler = Scheduler::new();
        ///
        /// // spawn a new task, returning a `JoinHandle`.
        /// let task = scheduler.spawn(async move {
        ///     // ... do stuff ...
        ///    42
        /// });
        ///
        /// // spawn another task that awaits the output of the first task.
        /// scheduler.spawn(async move {
        ///     // await the `JoinHandle` future, which completes when the task
        ///     // finishes, and unwrap its output.
        ///     let output = task.await.expect("task is not cancelled");
        ///     assert_eq!(output, 42);
        /// });
        ///
        /// // run the scheduler, driving the spawned tasks to completion.
        /// while scheduler.tick().has_remaining {}
        /// ```
        ///
        /// Spawning a task to run in the background, without awaiting its
        /// output:
        ///
        /// ```
        /// use maitake::scheduler::Scheduler;
        ///
        /// let scheduler = Scheduler::new();
        ///
        /// // dropping the `JoinHandle` allows the task to run in the background
        /// // without awaiting its output.
        /// scheduler.spawn(async move {
        ///     // ... do stuff ...
        /// });
        ///
        /// // run the scheduler, driving the spawned tasks to completion.
        /// while scheduler.tick().has_remaining {}
        /// ```
        ///
        /// [task]: crate::task
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            let (task, join) = TaskRef::new(self.clone(), future);
            self.0.spawn_inner(task);
            join
        }


        /// Spawn a pre-allocated task
        ///
        /// This method is used to spawn a task that requires some bespoke
        /// procedure of allocation, typically of a custom [`Storage`]
        /// implementor. See the documentation for the [`Storage`] trait for
        /// more details on using custom task storage.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](Self::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// [`Storage`]: crate::task::Storage
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn_allocated<F>(&'static self, task: Box<Task<Self, F, BoxStorage>>) -> JoinHandle<F::Output>
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            let (task, join) = TaskRef::new_allocated::<Self, F, BoxStorage>(self.clone(), task);
            self.0.spawn_inner(task);
            join
        }

        /// Returns a [`TaskRef`] referencing the task currently being polled by
        /// this scheduler, if a task is currently being polled.
        ///
        /// # Returns
        ///
        /// - [`Some`]`(`[`TaskRef`]`)` referencing the currently-polling task,
        ///   if a task is currently being polled (i.e., the scheduler is
        ///   [ticking](Self::tick) and the queue of scheduled tasks is
        ///   non-empty).
        ///
        /// - [`None`] if the scheduler is not currently being polled (i.e., the
        ///   scheduler is not ticking or its run queue is empty and all polls
        ///   have completed).
        #[must_use]
        #[inline]
        pub fn current_task(&self) -> Option<TaskRef> {
            self.0.current_task()
        }

        /// Tick this scheduler, polling up to [`Self::DEFAULT_TICK_SIZE`] tasks
        /// from the scheduler's run queue.
        ///
        /// Only a single CPU core/thread may tick a given scheduler at a time. If
        /// another call to `tick` is in progress on a different core, this method
        /// will immediately return.
        ///
        /// See [the module-level documentation][run-loops] for more information on
        /// using this function to implement a system's run loop.
        ///
        /// # Returns
        ///
        /// A [`Tick`] struct with data describing what occurred during the
        /// scheduler tick.
        ///
        /// [run-loops]: crate::scheduler#executing-tasks
        pub fn tick(&self) -> Tick {
            self.0.tick_n(Self::DEFAULT_TICK_SIZE)
        }
    }

    impl Schedule for Scheduler {
        fn schedule(&self, task: TaskRef) {
            self.0.wake(task)
        }

        #[must_use]
        fn current_task(&self) -> Option<TaskRef> {
            self.0.current_task()
        }
    }

    // === impl StaticScheduler ===

    impl StaticScheduler {
        /// Returns a new `StaticScheduler` with a heap-allocated stub task.
        ///
        /// Unlike [`StaticScheduler::new_with_static_stub`], this is *not* a
        /// `const fn`, as it performs a heap allocation for the stub task.
        /// However, the returned `StaticScheduler` must still be stored in a
        /// `static` variable in order to be used.
        ///
        /// This method is generally used with lazy initialization of the
        /// scheduler `static`.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Spawn a [task].
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](Self::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// # Examples
        ///
        /// Spawning a task and awaiting its output:
        ///
        /// ```
        /// use maitake::scheduler::{self, StaticScheduler};
        /// static SCHEDULER: StaticScheduler = scheduler::new_static!();
        ///
        /// // spawn a new task, returning a `JoinHandle`.
        /// let task = SCHEDULER.spawn(async move {
        ///     // ... do stuff ...
        ///    42
        /// });
        ///
        /// // spawn another task that awaits the output of the first task.
        /// SCHEDULER.spawn(async move {
        ///     // await the `JoinHandle` future, which completes when the task
        ///     // finishes, and unwrap its output.
        ///     let output = task.await.expect("task is not cancelled");
        ///     assert_eq!(output, 42);
        /// });
        ///
        /// // run the scheduler, driving the spawned tasks to completion.
        /// while SCHEDULER.tick().has_remaining {}
        /// ```
        ///
        /// Spawning a task to run in the background, without awaiting its
        /// output:
        ///
        /// ```
        /// use maitake::scheduler::{self, StaticScheduler};
        /// static SCHEDULER: StaticScheduler = scheduler::new_static!();
        ///
        /// // dropping the `JoinHandle` allows the task to run in the background
        /// // without awaiting its output.
        /// SCHEDULER.spawn(async move {
        ///     // ... do stuff ...
        /// });
        ///
        /// // run the scheduler, driving the spawned tasks to completion.
        /// while SCHEDULER.tick().has_remaining {}
        /// ```
        ///
        /// [task]: crate::task
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&'static self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            let (task, join) = TaskRef::new(self, future);
            self.0.spawn_inner(task);
            join
        }
    }

    // === impl LocalStaticScheduler ===

    impl LocalStaticScheduler {
        /// Returns a new `LocalStaticScheduler` with a heap-allocated stub task.
        ///
        /// Unlike [`LocalStaticScheduler::new_with_static_stub`], this is *not* a
        /// `const fn`, as it performs a heap allocation for the stub task.
        /// However, the returned `StaticScheduler` must still be stored in a
        /// `static` variable in order to be used.
        ///
        /// This method is generally used with lazy initialization of the
        /// scheduler `static`.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Spawn a [task].
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](Self::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// [task]: crate::task
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&'static self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new(self, future);
            self.core.spawn_inner(task);
            join
        }
    }

    // === impl LocalScheduler ===

    impl LocalScheduler {
        /// How many tasks are polled per call to `LocalScheduler::tick`.
        ///
        /// Chosen by fair dice roll, guaranteed to be random.
        pub const DEFAULT_TICK_SIZE: usize = Core::DEFAULT_TICK_SIZE;

        /// Returns a new `LocalScheduler`.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Returns a new [task `Builder`][`Builder`] for configuring tasks prior to spawning
        /// them on this scheduler.
        ///
        /// To spawn `!`[`Send`] tasks using a [`Builder`], use the
        /// [`Builder::spawn_local`](task::Builder::spawn_local) method.
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake::scheduler::LocalScheduler;
        ///
        /// let scheduler = LocalScheduler::new();
        /// scheduler.build_task().name("hello world").spawn_local(async {
        ///     // ...
        /// });
        ///
        /// scheduler.tick();
        /// ```
        ///
        /// Multiple tasks can be spawned using the same [`Builder`]:
        ///
        /// ```
        /// use maitake::scheduler::LocalScheduler;
        ///
        /// let scheduler = LocalScheduler::new();
        /// let builder = scheduler
        ///     .build_task()
        ///     .kind("my_cool_task");
        ///
        /// builder.spawn_local(async {
        ///     // ...
        /// });
        ///
        /// builder.spawn_local(async {
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

        /// Spawn a `!`[`Send`] [task].
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](Self::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// # Examples
        ///
        /// Spawning a task and awaiting its output:
        ///
        /// ```
        /// use maitake::scheduler::LocalScheduler;
        ///
        /// let scheduler = LocalScheduler::new();
        ///
        /// // spawn a new task, returning a `JoinHandle`.
        /// let task = scheduler.spawn(async move {
        ///     // ... do stuff ...
        ///    42
        /// });
        ///
        /// // spawn another task that awaits the output of the first task.
        /// scheduler.spawn(async move {
        ///     // await the `JoinHandle` future, which completes when the task
        ///     // finishes, and unwrap its output.
        ///     let output = task.await.expect("task is not cancelled");
        ///     assert_eq!(output, 42);
        /// });
        ///
        /// // run the scheduler, driving the spawned tasks to completion.
        /// while scheduler.tick().has_remaining {}
        /// ```
        ///
        /// Spawning a task to run in the background, without awaiting its
        /// output:
        ///
        /// ```
        /// use maitake::scheduler::LocalScheduler;
        ///
        /// let scheduler = LocalScheduler::new();
        ///
        /// // dropping the `JoinHandle` allows the task to run in the background
        /// // without awaiting its output.
        /// scheduler.spawn(async move {
        ///     // ... do stuff ...
        /// });
        ///
        /// // run the scheduler, driving the spawned tasks to completion.
        /// while scheduler.tick().has_remaining {}
        /// ```
        ///
        /// [task]: crate::task
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new(self.clone(), future);
            self.core.spawn_inner(task);
            join
        }


        /// Spawn a pre-allocated `!`[`Send`] task.
        ///
        /// This method is used to spawn a task that requires some bespoke
        /// procedure of allocation, typically of a custom [`Storage`]
        /// implementor. See the documentation for the [`Storage`] trait for
        /// more details on using custom task storage.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](Self::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// [`Storage`]: crate::task::Storage
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn_allocated<F>(&self, task: Box<Task<Self, F, BoxStorage>>) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new_allocated::<Self, F, BoxStorage>(self.clone(), task);
            self.core.spawn_inner(task);
            join
        }

        /// Returns a [`TaskRef`] referencing the task currently being polled by
        /// this scheduler, if a task is currently being polled.
        ///
        /// # Returns
        ///
        /// - [`Some`]`(`[`TaskRef`]`)` referencing the currently-polling task,
        ///   if a task is currently being polled (i.e., the scheduler is
        ///   [ticking](Self::tick) and the queue of scheduled tasks is
        ///   non-empty).
        ///
        /// - [`None`] if the scheduler is not currently being polled (i.e., the
        ///   scheduler is not ticking or its run queue is empty and all polls
        ///   have completed).
        #[must_use]
        #[inline]
        pub fn current_task(&self) -> Option<TaskRef> {
            self.core.current_task()
        }


        /// Tick this scheduler, polling up to [`Self::DEFAULT_TICK_SIZE`] tasks
        /// from the scheduler's run queue.
        ///
        /// Only a single CPU core/thread may tick a given scheduler at a time. If
        /// another call to `tick` is in progress on a different core, this method
        /// will immediately return.
        ///
        /// See [the module-level documentation][run-loops] for more information on
        /// using this function to implement a system's run loop.
        ///
        /// # Returns
        ///
        /// A [`Tick`] struct with data describing what occurred during the
        /// scheduler tick.
        ///
        /// [run-loops]: crate::scheduler#executing-tasks
        pub fn tick(&self) -> Tick {
            self.core.tick_n(Self::DEFAULT_TICK_SIZE)
        }

        /// Returns a new [`LocalSpawner`] that can be used by other threads to
        /// spawn [`Send`] tasks on this scheduler.
        #[must_use = "the returned `LocalSpawner` does nothing unless used to spawn tasks"]
        #[cfg(not(loom))] // Loom's `Arc` does not have a weak reference type...
        pub fn spawner(&self) -> LocalSpawner {
            LocalSpawner(Arc::downgrade(&self.core))
        }
    }

    impl Schedule for LocalScheduler {
        fn schedule(&self, task: TaskRef) {
            self.core.wake(task)
        }

        #[must_use]
        fn current_task(&self) -> Option<TaskRef> {
            self.core.current_task()
        }
    }

    // === impl LocalStaticSpawner ===

    impl LocalStaticSpawner {
        /// Spawn a task on the [`LocalStaticScheduler`] this handle
        /// references.
        ///
        /// Unlike [`LocalStaticScheduler::spawn`], this method requires that the
        /// spawned `Future` implement [`Send`], as the `LocalStaticSpawner` type is [`Send`]
        /// and [`Sync`], and therefore allows tasks to be spawned on a local
        /// scheduler from other threads.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](LocalStaticScheduler::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// [`Storage`]: crate::task::Storage
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + Send +'static,
            F::Output: Send + 'static,
        {
            self.0.spawn(future)
        }
    }

    // === impl LocalSpawner ===

    impl LocalSpawner {
        /// Spawn a task on the [`LocalScheduler`] this handle
        /// references.
        ///
        /// Unlike [`LocalScheduler::spawn`], this method requires that the
        /// spawned `Future` implement [`Send`], as the `LocalSpawner` type is [`Send`]
        /// and [`Sync`], and therefore allows tasks to be spawned on a local
        /// scheduler from other threads.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](LocalScheduler::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// [`Storage`]: crate::task::Storage
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        #[cfg(not(loom))]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + Send +'static,
            F::Output: Send + 'static,
        {
            match self.0.upgrade() {
                Some(core) => LocalScheduler { core, _not_send: PhantomData }.spawn(future),
                None => JoinHandle::error(task::join_handle::JoinErrorKind::Shutdown),
            }
        }

        /// Spawn a pre-allocated task on the [`LocalScheduler`] this handle
        /// references.
        ///
        /// Unlike [`LocalScheduler::spawn_allocated`] and
        /// [`LocalStaticScheduler::spawn_allocated`], this method requires that the
        /// spawned `Future` implement [`Send`], as the `LocalSpawner` type is [`Send`]
        /// and [`Sync`], and therefore allows tasks to be spawned on a local
        /// scheduler from other threads.
        ///
        /// This method is used to spawn a task that requires some bespoke
        /// procedure of allocation, typically of a custom [`Storage`] implementor.
        /// See the documentation for the [`Storage`] trait for more details on
        /// using custom task storage.
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        ///
        /// When tasks are spawned on a scheduler, the scheduler must be
        /// [ticked](LocalScheduler::tick) in order to drive those tasks to completion.
        /// See the [module-level documentation][run-loops] for more information
        /// on implementing a system's run loop.
        ///
        /// [`Storage`]: crate::task::Storage
        /// [run-loops]: crate::scheduler#executing-tasks
        #[inline]
        #[track_caller]
        #[cfg(not(loom))]
        pub fn spawn_allocated<F>(&self, task: Box<Task<LocalScheduler, F, BoxStorage>>) -> JoinHandle<F::Output>
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            match self.0.upgrade() {
                Some(core) => LocalScheduler { core, _not_send: PhantomData }.spawn_allocated(task),
                None => JoinHandle::error(task::join_handle::JoinErrorKind::Shutdown),
            }
        }
    }

    // === impl Core ===

    impl Core {
        fn new() -> Self {
            let stub_task = Box::new(Task::new_stub());
            let (stub_task, _) = TaskRef::new_allocated::<task::Stub, task::Stub, BoxStorage>(task::Stub, stub_task);
            Self {
                run_queue: MpscQueue::new_with_stub(test_dbg!(stub_task)),
                queued: AtomicUsize::new(0),
                current_task: AtomicPtr::new(ptr::null_mut()),
                spawned: AtomicUsize::new(0),
                woken: AtomicUsize::new(0),
            }
        }
    }

    impl Default for Core {
        fn default() -> Self {
            Self::new()
        }
    }
}
