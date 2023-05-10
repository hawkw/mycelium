use core::fmt;

/// A unique identifier for a running [task].
///
/// A `TaskId` is an opaque object that uniquely identifies each [task]
/// created during the lifetime of a program. `TaskIds`s are guaranteed not to
/// be reused, even when a task terminates. A `TaskId` can be retrieved from the
/// [`TaskRef::id`], [`JoinHandle::id`], and [`Task::id`] methods.
///
/// A `TaskId` does *not* increase the reference count of the [task] it
/// references, and task IDs may persist even after the task they identify has
/// completed and been deallocated.
///
/// [task]: crate::task
/// [`TaskRef::id`]: crate::task::TaskRef::id
/// [`JoinHandle::id`]: crate::task::JoinHandle::id
/// [`Task::id`]: crate::task::Task::id
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    pub(crate) fn next() -> Self {
        // Don't use loom atomics, since this has to go in a static.
        use portable_atomic::{AtomicU64, Ordering::Relaxed};

        // ID 0 is reserved for stub tasks.
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Relaxed);

        debug_assert!(id > 0, "64-bit task ID counter should not overflow!");
        Self(id)
    }

    #[must_use]
    #[inline]
    pub(crate) const fn stub() -> Self {
        Self(0)
    }

    #[allow(dead_code)]
    #[must_use]
    #[inline]
    pub(crate) fn is_stub(self) -> bool {
        self.0 == 0
    }

    #[inline]
    #[allow(dead_code)] // currently only used when tracing is enabled
    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::UpperHex for TaskId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskId(")?;
        fmt::UpperHex::fmt(&self.0, f)?;
        f.write_str(")")
    }
}

impl fmt::LowerHex for TaskId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskId(")?;
        fmt::LowerHex::fmt(&self.0, f)?;
        f.write_str(")")
    }
}

impl fmt::Debug for TaskId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskId(")?;
        fmt::Debug::fmt(&self.0, f)?;
        f.write_str(")")
    }
}

impl fmt::Display for TaskId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

// ==== PartialEq impls for refs ====

impl PartialEq<&'_ TaskId> for TaskId {
    #[inline]
    fn eq(&self, other: &&Self) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<TaskId> for &'_ TaskId {
    #[inline]
    fn eq(&self, other: &TaskId) -> bool {
        self.0 == other.0
    }
}
