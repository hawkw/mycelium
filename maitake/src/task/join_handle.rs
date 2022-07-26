use super::{Context, Poll, TaskRef};
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
/// [`Scheduler::spawn`]: crate::scheduler::Scheduler::spawn
/// [`Scheduler::spawn_allocated`]: crate::scheduler::Scheduler::spawn_allocated
/// [`task::Builder::spawn`]: crate::task::Builder::spawn
/// [`task::Builder::spawn_allocated`]: crate::task::Builder::spawn_allocated
/// [task]: crate::task
#[derive(Debug)]
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
        test_debug!(task = ?self.task, "drop JoinHandle");
        // if the JoinHandle has not already been consumed, clear the join
        // handle flag on the task.
        if let Some(ref task) = self.task {
            task.state().drop_join_handle();
        }
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

    /// Returns `true` if a task failed to join because it was canceled.
    pub fn is_canceled(&self) -> bool {
        matches!(self.kind, JoinErrorKind::Canceled)
    }
}
