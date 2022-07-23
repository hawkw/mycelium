use crate::task::{self, Header, JoinHandle, Storage, TaskRef};
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
            let _span = debug_span!("poll", ?task).entered();
            let poll = task.poll();
            if poll.is_ready() {
                tick.completed += 1;
            }
            tick.polled += 1;

            debug!(poll = ?poll, tick.polled, tick.completed);
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

        #[inline]
        #[track_caller]
        pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new(self.clone(), future);
            self.schedule(task);
            join
        }

        #[inline]
        #[track_caller]
        pub fn spawn_allocated<F>(&'static self, task: Box<Task<Self, F, BoxStorage>>) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new_allocated::<Self, F, BoxStorage>(task);
            self.schedule(task);
            join
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
        #[track_caller]
        pub fn spawn<F>(&'static self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let (task, join) = TaskRef::new(self, future);
            self.schedule(task);
            join
        }
    }

    impl Core {
        fn new() -> Self {
            let (stub_task, _) = TaskRef::new(Stub, Stub);
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

#[cfg(test)]
mod tests;
