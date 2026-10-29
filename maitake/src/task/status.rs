use super::{state::State, TaskRef};
use crate::{
    loom::sync::atomic::{AtomicUsize, Ordering},
    sync::WaitQueue,
};
use core::fmt;

/// A snapshot of a [task]'s status.
///
/// This type is returned by the [`TaskRef::status`], [`Task::status`], and
/// [`JoinHandle::status`] methods. It records what a task was doing at the
/// moment it was observed. A task which is running on another core may make
/// progress immediately afterwards, so a `TaskStatus` describes the past
/// rather than the present.
///
/// The individual predicates on this type report the raw facts recorded for
/// the task, several of which may hold at once. [`TaskStatus::phase`]
/// summarizes them as a single [`TaskPhase`] when only the broad strokes are
/// wanted, such as when logging.
///
/// [task]: crate::task
/// [`TaskRef::status`]: crate::task::TaskRef::status
/// [`Task::status`]: crate::task::Task::status
/// [`JoinHandle::status`]: crate::task::JoinHandle::status
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct TaskStatus(State);

/// A summary of the [phase] of a task's life that its [`TaskStatus`] describes.
///
/// This is returned by the [`TaskStatus::phase`] method.
///
/// Several of the facts a [`TaskStatus`] records can hold at the same time, so
/// a `TaskPhase` reports the first of these that applies, in the order the
/// variants are listed here.
///
/// [phase]: TaskStatus::phase
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TaskPhase {
    /// The task's [`Future`] has returned [`Poll::Ready`].
    ///
    /// A task which completed before it was canceled is `Complete` rather than
    /// [`Canceled`](Self::Canceled); it ran to the end, and the cancellation
    /// came too late to change that.
    ///
    /// [`Future`]: core::future::Future
    /// [`Poll::Ready`]: core::task::Poll::Ready
    Complete,

    /// The task has been canceled without having completed.
    ///
    /// Cancellation prevents new polls; an in-progress poll may still complete.
    Canceled,

    /// The task was being polled when it was observed.
    Running,

    /// The task has a wakeup which no poll has consumed yet, so a scheduler
    /// will poll it when it next runs.
    Scheduled,

    /// The task is waiting to be woken, and is not otherwise busy.
    Idle,
}

// === impl TaskStatus ===

impl TaskStatus {
    pub(super) fn new(state: State) -> Self {
        Self(state)
    }

    /// Returns the [`TaskPhase`] summarizing this status.
    ///
    /// Because more than one of this type's predicates can hold at once, the
    /// phase is the first of the following that applies: [`Complete`] if the
    /// task's future has returned, then [`Canceled`] if it has been canceled,
    /// then [`Running`] if it was being polled, then [`Scheduled`] if it has a
    /// pending wakeup, and otherwise [`Idle`].
    ///
    /// [`Complete`]: TaskPhase::Complete
    /// [`Canceled`]: TaskPhase::Canceled
    /// [`Running`]: TaskPhase::Running
    /// [`Scheduled`]: TaskPhase::Scheduled
    /// [`Idle`]: TaskPhase::Idle
    #[must_use]
    pub fn phase(&self) -> TaskPhase {
        if self.is_completed() {
            TaskPhase::Complete
        } else if self.is_canceled() {
            TaskPhase::Canceled
        } else if self.is_polling() {
            TaskPhase::Running
        } else if self.is_woken() {
            TaskPhase::Scheduled
        } else {
            TaskPhase::Idle
        }
    }

    /// Returns `true` if the task will not be polled again, because it has
    /// either completed or been canceled.
    #[inline]
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.is_completed() || self.is_canceled()
    }

    /// Returns `true` if the task was being polled when this status was taken.
    #[inline]
    #[must_use]
    pub fn is_polling(&self) -> bool {
        self.0.get(State::POLLING)
    }

    /// Returns `true` if the task has been woken, and the wakeup has not yet
    /// been consumed by a poll.
    #[inline]
    #[must_use]
    pub fn is_woken(&self) -> bool {
        self.0.get(State::WOKEN)
    }

    /// Returns `true` if the task's [`Future`] has returned [`Poll::Ready`].
    ///
    /// [`Future`]: core::future::Future
    /// [`Poll::Ready`]: core::task::Poll::Ready
    #[inline]
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.0.get(State::COMPLETED)
    }

    /// Returns `true` if the task has been canceled.
    ///
    /// A task is canceled as soon as [`TaskRef::cancel`] or
    /// [`JoinHandle::cancel`] is called for it, before the scheduler has
    /// noticed. Cancellation prevents subsequent polls; a poll already in
    /// progress may still return an output.
    ///
    /// [`TaskRef::cancel`]: crate::task::TaskRef::cancel
    /// [`JoinHandle::cancel`]: crate::task::JoinHandle::cancel
    #[inline]
    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.0.get(State::CANCELED)
    }

    /// Returns `true` if a [`JoinHandle`] for the task exists.
    ///
    /// [`JoinHandle`]: crate::task::JoinHandle
    #[inline]
    #[must_use]
    pub fn has_join_handle(&self) -> bool {
        self.0.get(State::HAS_JOIN_HANDLE)
    }

    /// Returns `true` if the task's output is waiting for a [`JoinHandle`] to
    /// take it.
    ///
    /// [`JoinHandle`]: crate::task::JoinHandle
    #[inline]
    #[must_use]
    pub fn has_output(&self) -> bool {
        self.0.get(State::HAS_OUTPUT)
    }

    /// Returns the number of live references to the task.
    #[inline]
    #[must_use]
    pub fn ref_count(&self) -> usize {
        self.0.ref_count()
    }
}

impl fmt::Debug for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskStatus")
            .field("phase", &self.phase())
            .field("polling", &self.is_polling())
            .field("woken", &self.is_woken())
            .field("completed", &self.is_completed())
            .field("canceled", &self.is_canceled())
            .field("has_join_handle", &self.has_join_handle())
            .field("has_output", &self.has_output())
            .field("ref_count", &self.ref_count())
            .finish()
    }
}

// === impl TaskPhase ===

impl TaskPhase {
    /// Returns the name of this phase, as it is written by this type's
    /// [`Display`] implementation.
    ///
    /// [`Display`]: core::fmt::Display
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskPhase::Complete => "complete",
            TaskPhase::Canceled => "canceled",
            TaskPhase::Running => "running",
            TaskPhase::Scheduled => "scheduled",
            TaskPhase::Idle => "idle",
        }
    }
}

impl fmt::Display for TaskPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

/// An independent cursor over a task's lifecycle progress.
///
/// Observers retain the task but never consume its output. Cloning preserves
/// the delivery position; subsequent deliveries advance each clone separately.
#[derive(Clone, Debug)]
pub struct TaskObserver {
    task: TaskRef,
    seen: Option<usize>,
    finished: bool,
}

#[derive(Debug)]
pub(super) struct Progress {
    generation: AtomicUsize,
    waiters: WaitQueue,
}

impl Progress {
    loom_const_fn! {
        pub(super) fn new() -> Self {
            Self { generation: AtomicUsize::new(0), waiters: WaitQueue::new() }
        }
    }

    pub(super) fn publish(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.waiters.wake_all();
    }
}

impl TaskObserver {
    pub(super) fn new(task: TaskRef) -> Self {
        Self {
            task,
            seen: None,
            finished: false,
        }
    }

    /// Reads a snapshot without acknowledging any lifecycle progress.
    #[must_use]
    pub fn status(&self) -> TaskStatus {
        self.task.status()
    }

    /// Waits for lifecycle progress without taking the task's output.
    ///
    /// The first delivery is immediate. Later deliveries coalesce completed
    /// polls and cancellation, including polls that leave the same status.
    /// After delivering a completed or canceled status, returns `None` forever.
    /// Dropping this future leaves the delivery position unchanged.
    pub async fn changed(&mut self) -> Option<TaskStatus> {
        if self.finished {
            return None;
        }
        let progress = &self.task.header().progress;
        let seen = self.seen;
        // Subscribe before checking the generation: a publication between a
        // check and registration must not leave an observer asleep forever.
        let (generation, status) = progress
            .waiters
            .wait_for_value(|| {
                let generation = progress.generation.load(Ordering::Acquire);
                if seen != Some(generation) {
                    Some((generation, self.task.status()))
                } else {
                    None
                }
            })
            .await
            .expect("task progress queue is never closed");
        self.seen = Some(generation);
        self.finished = status.is_finished();
        Some(status)
    }
}
