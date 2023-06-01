use super::{Context, Poll, TaskId, TaskRef};
use core::{future::Future, marker::PhantomData, pin::Pin};
use mycelium_util::fmt;

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
#[derive(PartialEq, Eq)]
// This clippy lint appears to be triggered incorrectly; this type *does* derive
// `Eq` based on its `PartialEq<Self>` impl, but it also implements `PartialEq`
// with types other than `Self` (which cannot impl `Eq`).
#[allow(clippy::derive_partial_eq_without_eq)]
pub struct JoinHandle<T> {
    task: JoinHandleState,
    id: TaskId,
    _t: PhantomData<fn(T)>,
}

/// Errors returned by awaiting a [`JoinHandle`].
#[derive(PartialEq, Eq)]
pub struct JoinError<T> {
    kind: JoinErrorKind,
    id: TaskId,
    output: Option<T>,
}

#[derive(PartialEq, Eq, Debug)]
enum JoinHandleState {
    Task(TaskRef),
    Empty,
    Error(JoinErrorKind),
}

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum JoinErrorKind {
    /// The task was canceled.
    Canceled {
        /// `true` if the task was canceled after it completed successfully.
        completed: bool,
    },

    /// A stub was awaited
    StubNever,

    /// The scheduler has been dropped.
    Shutdown,
}

impl<T> JoinHandle<T> {
    /// Converts a `TaskRef` into a `JoinHandle`.
    ///
    /// # Safety
    ///
    /// The pointed type must actually output a `T`-typed value.
    pub(super) unsafe fn from_task_ref(task: TaskRef) -> Self {
        task.state().create_join_handle();
        let id = task.id();
        Self {
            task: JoinHandleState::Task(task),
            id,
            _t: PhantomData,
        }
    }

    pub(crate) fn error(kind: JoinErrorKind) -> Self {
        Self {
            id: TaskId::stub(),
            task: JoinHandleState::Error(kind),
            _t: PhantomData,
        }
    }

    /// Returns a [`TaskRef`] referencing the task this [`JoinHandle`] is
    /// associated with.
    ///
    /// This increases the task's reference count; its storage is not
    /// deallocated until all such [`TaskRef`]s are dropped.
    #[must_use]
    pub fn task_ref(&self) -> TaskRef {
        match self.task {
            JoinHandleState::Task(ref task) => task.clone(),
            JoinHandleState::Empty => {
                panic!("`TaskRef` only taken while polling a `JoinHandle`; this is a bug")
            }
            JoinHandleState::Error(ref error) => panic!("`JoinHandle` errored: {error:?}"),
        }
    }

    /// Returns `true` if this task has completed.
    ///
    /// Tasks are considered completed when the spawned [`Future`] has returned
    /// [`Poll::Ready`], or if the task has been canceled by the [`cancel()`]
    /// method.
    ///
    /// **Note**: This method can return `false` after [`cancel()`] has
    /// been called. This is because calling `cancel` *begins* the process of
    /// cancelling a task. The task is not considered canceled until it has been
    /// polled by the scheduler after calling [`cancel()`].
    ///
    /// [`cancel()`]: Self::cancel
    #[inline]
    #[must_use]
    pub fn is_complete(&self) -> bool {
        match self.task {
            JoinHandleState::Task(ref task) => task.is_complete(),
            // if the `JoinHandle`'s `TaskRef` has been taken, we know the
            // `Future` impl for `JoinHandle` completed, and the task has
            // _definitely_ completed.
            _ => true,
        }
    }

    /// Forcibly cancel the task.
    ///
    /// Canceling a task sets a flag indicating that it has been canceled and
    /// should terminate. The next time a canceled task is polled by the
    /// scheduler, it will terminate instead of polling the inner [`Future`]. If
    /// the task has a [`JoinHandle`], that [`JoinHandle`] will complete with a
    /// [`JoinError`]. The task then will be deallocated once all
    /// [`JoinHandle`]s and [`TaskRef`]s referencing it have been dropped.
    ///
    /// This method returns `true` if the task was canceled successfully, and
    /// `false` if the task could not be canceled (i.e., it has already completed,
    /// has already been canceled, cancel culture has gone TOO FAR, et cetera).
    pub fn cancel(&self) -> bool {
        match self.task {
            JoinHandleState::Task(ref task) => task.cancel(),
            _ => false,
        }
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
        self.id
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, JoinError<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        let task = match core::mem::replace(&mut this.task, JoinHandleState::Empty) {
            JoinHandleState::Task(task) => task,
            JoinHandleState::Empty => {
                panic!("`TaskRef` only taken while polling a `JoinHandle`; this is a bug")
            }
            JoinHandleState::Error(kind) => {
                return Poll::Ready(Err(JoinError {
                    kind,
                    id: this.id,
                    output: None,
                }))
            }
        };
        let poll = unsafe {
            // Safety: the `JoinHandle` must have been constructed with the
            // task's actual output type!
            task.poll_join::<T>(cx)
        };
        if poll.is_pending() {
            this.task = JoinHandleState::Task(task);
        } else {
            // clear join interest
            task.state().drop_join_handle();
        }
        poll
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        // if the JoinHandle has not already been consumed, clear the join
        // handle flag on the task.
        if let JoinHandleState::Task(ref task) = self.task {
            test_debug!(
                task = ?self.task,
                task.tid = task.id().as_u64(),
                consumed = false,
                "drop JoinHandle",
            );
            task.state().drop_join_handle();
        } else {
            test_debug!(
                task = ?self.task,
                consumed = true,
                "drop JoinHandle",
            );
        }
    }
}

// ==== PartialEq impls for JoinHandle/TaskRef ====

impl<T> PartialEq<TaskRef> for JoinHandle<T> {
    fn eq(&self, other: &TaskRef) -> bool {
        match self.task {
            JoinHandleState::Task(ref task) => task == other,
            _ => false,
        }
    }
}

impl<T> PartialEq<&'_ TaskRef> for JoinHandle<T> {
    fn eq(&self, other: &&TaskRef) -> bool {
        match self.task {
            JoinHandleState::Task(ref task) => task == *other,
            _ => false,
        }
    }
}

impl<T> PartialEq<JoinHandle<T>> for TaskRef {
    fn eq(&self, other: &JoinHandle<T>) -> bool {
        match other.task {
            JoinHandleState::Task(ref task) => self == task,
            _ => false,
        }
    }
}

impl<T> PartialEq<&'_ JoinHandle<T>> for TaskRef {
    fn eq(&self, other: &&JoinHandle<T>) -> bool {
        match other.task {
            JoinHandleState::Task(ref task) => self == task,
            _ => false,
        }
    }
}

// ==== PartialEq impls for JoinHandle/TaskId ====

impl<T> PartialEq<TaskId> for JoinHandle<T> {
    #[inline]
    fn eq(&self, other: &TaskId) -> bool {
        self.id == other
    }
}

impl<T> PartialEq<&'_ TaskId> for JoinHandle<T> {
    #[inline]
    fn eq(&self, other: &&TaskId) -> bool {
        self.id == *other
    }
}

impl<T> PartialEq<JoinHandle<T>> for TaskId {
    #[inline]
    fn eq(&self, other: &JoinHandle<T>) -> bool {
        self == other.id
    }
}

impl<T> PartialEq<&'_ JoinHandle<T>> for TaskId {
    #[inline]
    fn eq(&self, other: &&JoinHandle<T>) -> bool {
        self == other.id
    }
}

impl<T> fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinHandle")
            .field("output", &core::any::type_name::<T>())
            .field("task", &self.task)
            .field("id", &self.id)
            .finish()
    }
}

// === impl JoinError ===

impl JoinError<()> {
    #[inline]
    pub(super) fn canceled(completed: bool, id: TaskId) -> Poll<Result<(), Self>> {
        Poll::Ready(Err(Self {
            kind: JoinErrorKind::Canceled { completed },
            id,
            output: None,
        }))
    }

    #[allow(dead_code)]
    #[inline]
    pub(crate) fn stub() -> Self {
        Self {
            kind: JoinErrorKind::StubNever,
            id: TaskId::stub(),
            output: None,
        }
    }

    #[must_use]
    pub(super) fn with_output<T>(self, output: Option<T>) -> JoinError<T> {
        JoinError {
            kind: self.kind,
            id: self.id,
            output,
        }
    }
}

impl<T> JoinError<T> {
    /// Returns `true` if a task failed to join because it was canceled.
    pub fn is_canceled(&self) -> bool {
        matches!(self.kind, JoinErrorKind::Canceled { .. })
    }

    /// Returns `true` if the task completed successfully before it was canceled.
    pub fn is_completed(&self) -> bool {
        match self.kind {
            JoinErrorKind::Canceled { completed } => completed,
            _ => false,
        }
    }

    /// Returns the [`TaskId`] of the task that failed to join.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the task's output, if the task completed successfully before it
    /// was canceled.
    ///
    /// Otherwise, returns `None`.
    pub fn output(self) -> Option<T> {
        self.output
    }
}

impl<T> fmt::Display for JoinError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            JoinErrorKind::Canceled { completed } => {
                let completed = if completed {
                    " (after completing successfully)"
                } else {
                    ""
                };
                write!(f, "task {} was canceled{completed}", self.id)
            }
            JoinErrorKind::StubNever => f.write_str("the stub task can never join"),
            JoinErrorKind::Shutdown => f.write_str("the scheduler has already shut down"),
        }
    }
}

impl<T> fmt::Debug for JoinError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinError")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .finish()
    }
}

feature! {
    #![feature = "core-error"]
    impl<T> core::error::Error for JoinError<T> {}
}
