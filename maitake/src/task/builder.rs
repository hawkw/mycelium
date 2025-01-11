use crate::scheduler::LocalStaticScheduler;

use super::{Future, JoinHandle, Schedule, Storage, TaskRef};
use core::panic::Location;

/// Builds a new [`Task`] prior to spawning it.
///
/// [`Task`]: crate::task::Task
#[derive(Debug, Clone)]
pub struct Builder<'a, S> {
    scheduler: S,
    settings: Settings<'a>,
}

/// Configures settings for new tasks.
#[derive(Debug, Clone)]
// These fields are currently only read when tracing is enabled.
#[cfg_attr(
    not(any(feature = "tracing-01", feature = "tracing-02", test)),
    allow(dead_code)
)]
pub(crate) struct Settings<'a> {
    pub(super) name: Option<&'a str>,
    pub(super) kind: &'static str,
    pub(super) location: Option<Location<'a>>,
}

impl<'a, S: Schedule + 'static> Builder<'a, S> {
    pub(crate) const fn new(scheduler: S) -> Self {
        Self {
            scheduler,
            settings: Settings::new(),
        }
    }

    /// Adds a name to the tasks produced by this builder.
    ///
    /// This will set the `task.name` `tracing` field of spans generated for
    /// this task, if the "tracing-01" or "tracing-02" feature flags are
    /// enabled.
    ///
    /// By default, tasks are unnamed.
    pub fn name(self, name: &'a str) -> Self {
        Self {
            settings: Settings {
                name: Some(name),
                ..self.settings
            },
            ..self
        }
    }

    /// Adds a static string which describes the type of the configured task.
    ///
    /// Generally, this is set by the runtime, rather than by user code &mdash;
    /// `kind`s should describe general categories of task, such as "local" or
    /// "blocking", rather than identifying specific tasks in an application. The
    /// [`name`] field should be used instead for naming specific tasks within
    /// an application.
    ///
    /// This will set the `task.kind` `tracing` field of spans generated for
    /// this task, if the "tracing-01" or "tracing-02" feature flags are
    /// enabled.
    ///
    /// By default, tasks will have the kind "task".
    ///
    /// [`name`]: Self::name
    pub fn kind(self, kind: &'static str) -> Self {
        Self {
            settings: Settings {
                kind,
                ..self.settings
            },
            ..self
        }
    }

    /// Overrides the task's source code location.
    ///
    /// By default, tasks will be recorded as having the location from which
    /// they are spawned. This may be overriden by the runtime if needed.
    pub fn location(self, location: Location<'a>) -> Self {
        Self {
            settings: Settings {
                location: Some(location),
                ..self.settings
            },
            ..self
        }
    }

    /// Spawns a new task in a custom allocation, with this builder's configured settings.
    ///
    /// Note that the `StoredTask` *must* be bound to the same scheduler
    /// instance as this task's scheduler!
    ///
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    #[inline]
    #[track_caller]
    pub fn spawn_allocated<STO, F>(&self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        STO: Storage<S, F>,
    {
        let (task, join) = TaskRef::build_allocated::<S, F, STO>(&self.settings, task);
        self.scheduler.schedule(task);
        join
    }
}

impl Builder<'_, &'static LocalStaticScheduler> {
    /// Spawns a new `!`[`Send`] task in a custom allocation, with this
    /// builder's configured settings.
    ///
    /// This method is capable of spawning futures which do not implement
    /// `Send`. Therefore, it is only available when this [`Builder`] was
    /// returned by a [`LocalStaticScheduler`]
    ///
    /// Note that the `StoredTask` *must* be bound to the same scheduler
    /// instance as this task's scheduler!
    ///
    /// This method returns a [`JoinHandle`] that can be used to await the
    /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
    /// allowing it to run in the background without awaiting its output.
    #[inline]
    #[track_caller]
    pub fn spawn_local_allocated<STO, F>(&self, task: STO::StoredTask) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
        STO: Storage<&'static LocalStaticScheduler, F>,
    {
        let (task, join) =
            TaskRef::build_allocated::<&'static LocalStaticScheduler, F, STO>(&self.settings, task);
        self.scheduler.schedule(task);
        join
    }
}

feature! {
    #![feature = "alloc"]
    use alloc::boxed::Box;
    use super::{BoxStorage, Task};
    use crate::scheduler::LocalScheduler;

    impl<S: Schedule + 'static> Builder<'_, S> {
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
            let mut task = Box::new(Task::<S, _, BoxStorage>::new(future));
            task.bind(self.scheduler.clone());
            let (task, join) = TaskRef::build_allocated::<S, _, BoxStorage>(&self.settings, task);
            self.scheduler.schedule(task);
            join
        }
    }

    impl Builder<'_, &'static LocalStaticScheduler> {
        /// Spawns a new `!`[`Send`] task with this builder's configured settings.
        ///
        /// This method is capable of spawning futures which do not implement
        /// [`Send`]. Therefore, it is only available when this [`Builder`] was
        /// returned by a [`LocalStaticScheduler`]
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        #[inline]
        #[track_caller]
        pub fn spawn_local<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let mut task = Box::new(Task::<&'static LocalStaticScheduler, _, BoxStorage>::new(future));
            task.bind(self.scheduler);
            let (task, join) = TaskRef::build_allocated::<&'static LocalStaticScheduler, _, BoxStorage>(&self.settings, task);
            self.scheduler.schedule(task);
            join
        }
    }

    impl Builder<'_, LocalScheduler> {
        /// Spawns a new `!`[`Send`] task with this builder's configured settings.
        ///
        /// This method is capable of spawning futures which do not implement
        /// [`Send`]. Therefore, it is only available when this [`Builder`] was
        /// returned by a [`LocalScheduler`]
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        #[inline]
        #[track_caller]
        pub fn spawn_local<F>(&self, future: F) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
        {
            let mut task = Box::new(Task::<LocalScheduler, _, BoxStorage>::new(future));
            task.bind(self.scheduler.clone());
            let (task, join) = TaskRef::build_allocated::<LocalScheduler, _, BoxStorage>(&self.settings, task);
            self.scheduler.schedule(task);
            join
        }

        /// Spawns a new `!`[`Send`] task in a custom allocation, with this
        /// builder's configured settings.
        ///
        /// This method is capable of spawning futures which do not implement
        /// `Send`. Therefore, it is only available when this [`Builder`] was
        /// returned by a [`LocalStaticScheduler`]
        ///
        /// Note that the `StoredTask` *must* be bound to the same scheduler
        /// instance as this task's scheduler!
        ///
        /// This method returns a [`JoinHandle`] that can be used to await the
        /// task's output. Dropping the [`JoinHandle`] _detaches_ the spawned task,
        /// allowing it to run in the background without awaiting its output.
        #[inline]
        #[track_caller]
        pub fn spawn_local_allocated<STO, F>(&self, task: STO::StoredTask) -> JoinHandle<F::Output>
        where
            F: Future + 'static,
            F::Output: 'static,
            STO: Storage<LocalScheduler, F>,
        {
            let (task, join) = TaskRef::build_allocated::<LocalScheduler, F, STO>(&self.settings, task);
            self.scheduler.schedule(task);
            join
        }
    }
}

// === impl Settings ===

impl Settings<'_> {
    /// Returns a new, empty task builder with no settings configured.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            name: None,
            location: None,
            kind: "task",
        }
    }
}

impl Default for Settings<'_> {
    fn default() -> Self {
        Self::new()
    }
}
