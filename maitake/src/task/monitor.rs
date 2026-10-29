use super::{TaskId, TaskObserver, TaskRef, TaskStatus};
use alloc::{boxed::Box, vec::Vec};
use core::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// A task identity and a non-owning status snapshot.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TaskUpdate {
    /// The task whose state was observed.
    pub id: TaskId,
    /// The state at the observation point.
    pub status: TaskStatus,
}

/// A membership change exceeding a monitor's configured capacity.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MonitorFull {
    capacity: usize,
    required: usize,
}

impl MonitorFull {
    /// Returns the configured membership limit.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of distinct tasks the operation would retain.
    #[must_use]
    pub fn required(&self) -> usize {
        self.required
    }
}

impl fmt::Display for MonitorFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} tasks exceed monitor capacity {}",
            self.required, self.capacity
        )
    }
}

/// A bounded collection of independent task observation cursors.
///
/// Membership order controls delivery priority. Delivering a batch rotates
/// that priority past its last update, preventing a busy member from starving
/// a later ready member. Finished members remain available for inspection.
#[derive(Clone, Debug)]
pub struct TaskMonitor {
    entries: Vec<Entry>,
    capacity: usize,
    next_scan: usize,
}

#[derive(Clone, Debug)]
struct Entry {
    id: TaskId,
    observer: TaskObserver,
    exhausted: bool,
}

impl Entry {
    fn new(task: &TaskRef) -> Self {
        Self {
            id: task.id(),
            observer: task.observe(),
            exhausted: false,
        }
    }

    fn snapshot(&self) -> TaskUpdate {
        TaskUpdate {
            id: self.id,
            status: self.observer.status(),
        }
    }
}

impl TaskMonitor {
    /// Creates an empty monitor with a fixed membership limit.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
            next_scan: 0,
        }
    }

    /// Returns the maximum number of distinct retained tasks.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of retained tasks, including exhausted members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether there are no retained tasks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Counts members whose finished snapshot has not yet been delivered.
    #[must_use]
    pub fn active_len(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.exhausted).count()
    }

    /// Adds a task at the end of the membership order.
    ///
    /// An existing identity is a no-op, even at capacity. Failure leaves all
    /// cursors, ownership and delivery priority unchanged.
    pub fn subscribe(&mut self, task: &TaskRef) -> Result<bool, MonitorFull> {
        if self.entries.iter().any(|entry| entry.id == task.id()) {
            return Ok(false);
        }
        let required = self.len() + 1;
        self.check_capacity(required)?;
        self.entries.push(Entry::new(task));
        Ok(true)
    }

    /// Removes one task without consuming its output or canceling it.
    ///
    /// The next surviving member retains priority if the next member is
    /// removed; otherwise the current priority is preserved.
    pub fn remove(&mut self, id: TaskId) -> bool {
        let index = match self.entries.iter().position(|entry| entry.id == id) {
            Some(index) => index,
            None => return false,
        };
        self.entries.remove(index);
        if index < self.next_scan {
            self.next_scan -= 1;
        }
        if self.next_scan >= self.entries.len() {
            self.next_scan = 0;
        }
        true
    }

    /// Releases every membership. The monitor can be reused afterwards.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.next_scan = 0;
    }

    /// Replaces membership, deduplicating identities in input order.
    ///
    /// Existing cursors are moved, not restarted. All validation completes
    /// before any membership or cursor changes. Successful replacement starts
    /// scanning at the first member of the new order.
    pub fn reconcile(&mut self, tasks: &[TaskRef]) -> Result<(), MonitorFull> {
        let mut distinct = Vec::new();
        for task in tasks {
            if !distinct.iter().any(|old: &&TaskRef| old.id() == task.id()) {
                distinct.push(task);
            }
        }
        self.check_capacity(distinct.len())?;
        let mut replacement = Vec::with_capacity(distinct.len());
        for task in distinct {
            let entry = match self.entries.iter().position(|entry| entry.id == task.id()) {
                Some(index) => self.entries.remove(index),
                None => Entry::new(task),
            };
            replacement.push(entry);
        }
        self.entries = replacement;
        self.next_scan = 0;
        Ok(())
    }

    /// Reads one retained task without advancing its cursor.
    #[must_use]
    pub fn status(&self, id: TaskId) -> Option<TaskStatus> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.observer.status())
    }

    /// Reads all retained tasks in membership order without acknowledging events.
    #[must_use]
    pub fn snapshot(&self) -> Vec<TaskUpdate> {
        self.entries.iter().map(Entry::snapshot).collect()
    }

    /// Waits for one update, or returns `None` when every cursor is exhausted.
    pub async fn next(&mut self) -> Option<TaskUpdate> {
        self.next_batch(1).await.pop()
    }

    /// Waits for up to `limit` updates in rotating membership order.
    ///
    /// A partial batch returns immediately. Each member contributes at most
    /// once, and unselected cursors are untouched. Zero is a no-op. Dropping
    /// this future removes all pending notifications without losing progress.
    pub async fn next_batch(&mut self, limit: usize) -> Vec<TaskUpdate> {
        if limit == 0 || self.active_len() == 0 {
            return Vec::new();
        }
        let count = self.entries.len();
        let start = self.next_scan;
        let (updates, next_scan) = {
            // Each borrowed future stays pinned for the whole wait. Its drop
            // unregisters notification, including waits not selected for output.
            let mut waits: Vec<_> = self
                .entries
                .iter_mut()
                .map(|entry| {
                    let future: Pin<Box<dyn Future<Output = Option<TaskStatus>> + Send + '_>> =
                        Box::pin(entry.observer.changed());
                    (entry.id, Some(future))
                })
                .collect();
            poll_updates(|cx| {
                let mut updates = Vec::new();
                let mut next_scan = start;
                let mut pending = false;
                for index in (start..count).chain(0..start) {
                    let (id, future) = &mut waits[index];
                    let wait = match future {
                        Some(wait) => wait,
                        None => continue,
                    };
                    match wait.as_mut().poll(cx) {
                        Poll::Pending => pending = true,
                        Poll::Ready(None) => *future = None,
                        Poll::Ready(Some(status)) => {
                            updates.push(TaskUpdate { id: *id, status });
                            *future = None;
                            next_scan = if index + 1 == count { 0 } else { index + 1 };
                            if updates.len() == limit {
                                break;
                            }
                        }
                    }
                }
                if updates.is_empty() && pending {
                    Poll::Pending
                } else {
                    Poll::Ready((updates, next_scan))
                }
            })
            .await
        };
        for update in &updates {
            if update.status.is_finished() {
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == update.id) {
                    entry.exhausted = true;
                }
            }
        }
        self.next_scan = next_scan;
        updates
    }

    fn check_capacity(&self, required: usize) -> Result<(), MonitorFull> {
        if required > self.capacity {
            return Err(MonitorFull {
                capacity: self.capacity,
                required,
            });
        }
        Ok(())
    }
}

// Keep this adapter local until core::future::poll_fn is available at the MSRV.
struct PollUpdates<F>(F);

// The callback is never exposed as structurally pinned.
impl<F> Unpin for PollUpdates<F> {}

impl<F, T> Future for PollUpdates<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<T>,
{
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        (self.0)(cx)
    }
}

fn poll_updates<F, T>(callback: F) -> PollUpdates<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<T>,
{
    PollUpdates(callback)
}
