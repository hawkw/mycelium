use super::{Context, Poll, TaskId, TaskRef};
use core::{future::Future, marker::PhantomData, pin::Pin};

/// An owned permission to join a [task] (await its termination).
///
/// This is equivalent to the standard library's [`std::thread::JoinHandle`]
/// type, but for asynchronous tasks rather than OS threads.
///
/// A `JoinHandle` *detaches* the associated task when it is dropped, which
/// means that there is no longer any handle to the task and no way to await
/// its termination.
///
/// `JoinHandle`s implement [`Future`], so a task's output can be awaited by
/// `.await`ing its `JoinHandle`.
///
/// This `struct` is returned by the [`Scheduler::spawn`] and
/// [`Scheduler::spawn_allocated`] methods, and the [`task::Builder::spawn`] and
/// [`task::Builder::spawn_allocated`] methods.
///
/// [task]: crate::task
/// [`std::thread::JoinHandle`]: https://doc.rust-lang.org/stable/std/thread/struct.JoinHandle.html
/// [`Scheduler::spawn`]: crate::scheduler::Scheduler::spawn
/// [`Scheduler::spawn_allocated`]: crate::scheduler::Scheduler::spawn_allocated
/// [`task::Builder::spawn`]: crate::task::Builder::spawn
/// [`task::Builder::spawn_allocated`]: crate::task::Builder::spawn_allocated
#[derive(Debug, PartialEq, Eq)]
// This clippy lint appears to be triggered incorrectly; this type *does* derive
// `Eq` based on its `PartialEq<Self>` impl, but it also implements `PartialEq`
// with types other than `Self` (which cannot impl `Eq`).
#[allow(clippy::derive_partial_eq_without_eq)]
pub struct JoinHandle<T> {
    task: Option<TaskRef>,
    _t: PhantomData<fn(T)>,
}

/// Errors returned by awaiting a [`JoinHandle`].
#[derive(Debug, PartialEq, Eq)]
pub struct JoinError {
    kind: JoinErrorKind,
}

#[allow(dead_code)] // this will be used when i implement task cancellation
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
enum JoinErrorKind {
    /// The task was canceled.
    Canceled,

    /// A stub was awaited
    StubNever,
}

impl<T> JoinHandle<T> {
    /// Converts a `TaskRef` into a `JoinHandle`.
    ///
    /// # Safety
    ///
    /// The pointed type must actually output a `T`-typed value.
    pub(super) unsafe fn from_task_ref(task: TaskRef) -> Self {
        task.state().create_join_handle();
        Self {
            task: Some(task),
            _t: PhantomData,
        }
    }

    /// Borrows the [`TaskRef`] referencing the task this [`JoinHandle`] is
    /// associated with.
    ///
    /// This does not increase the task's reference count. The returned
    /// `&TaskRef` can be cloned, returning a new [`TaskRef`], and increasing
    /// the task's reference count. The task is not deallocated until all such
    /// [`TaskRef`]s are dropped.
    #[must_use]
    #[inline]
    #[track_caller]
    pub fn task_ref(&self) -> &TaskRef {
        self.task
            .as_ref()
            .expect("`TaskRef` only taken while polling a `JoinHandle`; this is a bug")
    }

    /// Returns a [`TaskId`] that uniquely identifies this [task].
    ///
    /// The returned ID does *not* increment the task's reference count, and may
    /// persist even after the task it identifies has completed and been
    /// deallocated.
    ///
    /// [task]: crate::task
    #[must_use]
    #[inline]
    #[track_caller]
    pub fn id(&self) -> TaskId {
        self.task_ref().id()
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        let task = this.task.take().unwrap();
        let poll = unsafe {
            // Safety: the `JoinHandle` must have been constructed with the
            // task's actual output type!
            task.poll_join::<T>(cx)
        };
        if poll.is_pending() {
            this.task = Some(task);
        } else {
            // clear join interest
            task.state().drop_join_handle();
        }
        poll
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        test_debug!(task = ?self.task, task.tid = %self.id(), "drop JoinHandle");
        // if the JoinHandle has not already been consumed, clear the join
        // handle flag on the task.
        if let Some(ref task) = self.task {
            task.state().drop_join_handle();
        }
    }
}

impl<T> PartialEq<TaskRef> for JoinHandle<T> {
    fn eq(&self, other: &TaskRef) -> bool {
        self.task.as_ref().unwrap() == other
    }
}

impl<T> PartialEq<&'_ TaskRef> for JoinHandle<T> {
    fn eq(&self, other: &&TaskRef) -> bool {
        self.task.as_ref().unwrap() == *other
    }
}

impl<T> PartialEq<JoinHandle<T>> for TaskRef {
    fn eq(&self, other: &JoinHandle<T>) -> bool {
        self == other.task.as_ref().unwrap()
    }
}

impl<T> PartialEq<&'_ JoinHandle<T>> for TaskRef {
    fn eq(&self, other: &&JoinHandle<T>) -> bool {
        self == other.task.as_ref().unwrap()
    }
}

// === impl JoinError ===

impl JoinError {
    #[allow(dead_code)] // this will be used when i implement task cancellation
    #[inline]
    pub(crate) fn canceled() -> Self {
        Self {
            kind: JoinErrorKind::Canceled,
        }
    }

    #[allow(dead_code)] // this will be used when i implement task cancellation
    #[inline]
    pub(crate) fn stub() -> Self {
        Self {
            kind: JoinErrorKind::StubNever,
        }
    }

    /// Returns `true` if a task failed to join because it was canceled.
    pub fn is_canceled(&self) -> bool {
        matches!(self.kind, JoinErrorKind::Canceled)
    }
}
