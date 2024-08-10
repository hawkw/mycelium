//! An asynchronous [counting semaphore].
//!
//! A semaphore limits the number of tasks which may execute concurrently. See
//! the [`Semaphore`] type's documentation for details.
//!
//! [counting semaphore]: https://en.wikipedia.org/wiki/Semaphore_(programming)
use crate::{
    blocking::RawMutex,
    loom::{
        cell::UnsafeCell,
        sync::{
            atomic::{AtomicUsize, Ordering::*},
            blocking::{Mutex, MutexGuard},
        },
    },
    spin::Spinlock,
    util::{fmt, CachePadded, WakeBatch},
    WaitResult,
};
use cordyceps::{
    list::{self, List},
    Linked,
};
use core::{
    cmp,
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    ptr::{self, NonNull},
    task::{Context, Poll, Waker},
};
use pin_project::{pin_project, pinned_drop};

#[cfg(test)]
mod tests;

/// An asynchronous [counting semaphore].
///
/// A semaphore is a synchronization primitive that limits the number of tasks
/// that may run concurrently. It consists of a count of _permits_, which tasks
/// may [`acquire`] in order to execute in some context. When a task acquires a
/// permit from the semaphore, the count of permits held by the semaphore is
/// decreased. When no permits remain in the semaphore, any task that wishes to
/// acquire a permit must (asynchronously) wait until another task has released
/// a permit.
///
/// The [`Permit`] type is a RAII guard representing one or more permits
/// acquired from a `Semaphore`. When a [`Permit`] is dropped, the permits it
/// represents are released back to the `Semaphore`, potentially allowing a
/// waiting task to acquire them.
///
/// # Fairness
///
/// This semaphore is _fair_: as permits become available, they are assigned to
/// waiting tasks in the order that those tasks requested permits (first-in,
/// first-out). This means that all tasks waiting to acquire permits will
/// eventually be allowed to progress, and a single task cannot starve the
/// semaphore of permits (provided that permits are eventually released). The
/// semaphore remains fair even when a call to `acquire` requests more than one
/// permit at a time.
///
/// # Overriding the blocking mutex
///
/// This type uses a [blocking `Mutex`](crate::blocking::Mutex) internally to
/// synchronize access to its wait list. By default, this is a [`Spinlock`]. To
/// use an alternative [`RawMutex`] implementation, use the
/// [`new_with_raw_mutex`](Self::new_with_raw_mutex) constructor. See [the documentation
/// on overriding mutex
/// implementations](crate::blocking#overriding-mutex-implementations) for more
/// details.
///
/// Note that this type currently requires that the raw mutex implement
/// [`RawMutex`] rather than [`mutex_traits::ScopedRawMutex`]!
///
/// # Examples
///
/// Using a semaphore to limit concurrency:
///
/// ```
/// # use tokio::task;
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn test() {
/// # use std as alloc;
/// use maitake_sync::Semaphore;
/// use alloc::sync::Arc;
///
/// # let mut tasks = Vec::new();
/// // Allow 4 tasks to run concurrently at a time.
/// let semaphore = Arc::new(Semaphore::new(4));
///
/// for _ in 0..8 {
///     // Clone the `Arc` around the semaphore.
///     let semaphore = semaphore.clone();
///     # let t =
///     task::spawn(async move {
///         // Acquire a permit from the semaphore, returning a RAII guard that
///         // releases the permit back to the semaphore when dropped.
///         //
///         // If all 4 permits have been acquired, the calling task will yield,
///         // and it will be woken when another task releases a permit.
///         let _permit = semaphore
///             .acquire(1)
///             .await
///             .expect("semaphore will not be closed");
///
///         // do some work...
///     });
///     # tasks.push(t);
/// }
/// # for task in tasks { task.await.unwrap() };
/// # }
/// # test();
/// ```
///
/// A semaphore may also be used to cause a task to run once all of a set of
/// tasks have completed. If we want some task _B_ to run only after a fixed
/// number _n_ of tasks _A_ have run, we can have task _B_ try to acquire _n_
/// permits from a semaphore with 0 permits, and have each task _A_ add one
/// permit to the semaphore when it completes.
///
/// For example:
///
/// ```
/// # use tokio::task;
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn test() {
/// # use std as alloc;
/// use maitake_sync::Semaphore;
/// use alloc::sync::Arc;
///
/// // How many tasks will we be waiting for the completion of?
/// const TASKS: usize = 4;
///
/// // Create the semaphore with 0 permits.
/// let semaphore = Arc::new(Semaphore::new(0));
///
/// // Spawn the "B" task that will wait for the 4 "A" tasks to complete.
/// # let b_task =
/// task::spawn({
///     let semaphore = semaphore.clone();
///     async move {
///         println!("Task B starting...");
///
///         // Since the semaphore is created with 0 permits, this will
///         // wait until all 4 "A" tasks have completed.
///        let _permit = semaphore
///             .acquire(TASKS)
///             .await
///             .expect("semaphore will not be closed");
///
///         // ... do some work ...
///
///         println!("Task B done!");
///     }
/// });
///
/// # let mut tasks = Vec::new();
/// for i in 0..TASKS {
///     let semaphore = semaphore.clone();
///     # let t =
///     task::spawn(async move {
///         println!("Task A {i} starting...");
///
///         // Add a single permit to the semaphore. Once all 4 tasks have
///         // completed, the semaphore will have the 4 permits required to
///         // wake the "B" task.
///         semaphore.add_permits(1);
///
///         // ... do some work ...
///
///         println!("Task A {i} done");
///     });
///     # tasks.push(t);
/// }
///
/// # for t in tasks { t.await.unwrap() };
/// # b_task.await.unwrap();
/// # }
/// # test();
/// ```
///
/// [counting semaphore]: https://en.wikipedia.org/wiki/Semaphore_(programming)
/// [`acquire`]: Semaphore::acquire
#[derive(Debug)]
pub struct Semaphore<Lock: RawMutex = Spinlock> {
    /// The number of permits in the semaphore (or [`usize::MAX] if the
    /// semaphore is closed.
    permits: CachePadded<AtomicUsize>,

    /// The queue of tasks waiting to acquire permits.
    ///
    /// A spinlock (from `mycelium_util`) is used here, in order to support
    /// `no_std` platforms; when running `loom` tests, a `loom` mutex is used
    /// instead to simulate the spinlock, because loom doesn't play nice with
    /// real spinlocks.
    waiters: Mutex<SemQueue, Lock>,
}

/// A [RAII guard] representing one or more permits acquired from a
/// [`Semaphore`].
///
/// When the `Permit` is dropped, the permits it represents are released back to
/// the [`Semaphore`], potentially waking another task.
///
/// This type is returned by the [`Semaphore::acquire`] and
/// [`Semaphore::try_acquire`] methods.
///
/// [RAII guard]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
#[derive(Debug)]
#[must_use = "dropping a `Permit` releases the acquired permits back to the `Semaphore`"]
pub struct Permit<'sem, Lock: RawMutex = Spinlock> {
    permits: usize,
    semaphore: &'sem Semaphore<Lock>,
}

/// The future returned by the [`Semaphore::acquire`] method.
///
/// # Notes
///
/// This future is `!Unpin`, as it is unsafe to [`core::mem::forget`] an
/// `Acquire` future once it has been polled. For instance, the following code
/// must not compile:
///
///```compile_fail
/// use maitake_sync::semaphore::Acquire;
///
/// // Calls to this function should only compile if `T` is `Unpin`.
/// fn assert_unpin<T: Unpin>() {}
///
/// assert_unpin::<Acquire<'_>>();
/// ```
#[derive(Debug)]
#[pin_project(PinnedDrop)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Acquire<'sem, Lock: RawMutex = Spinlock> {
    semaphore: &'sem Semaphore<Lock>,
    queued: bool,
    permits: usize,
    #[pin]
    waiter: Waiter,
}

/// Errors returned by [`Semaphore::try_acquire`].

#[derive(Debug, PartialEq, Eq)]
pub enum TryAcquireError {
    /// The semaphore has been [closed], so additional permits cannot be
    /// acquired.
    ///
    /// [closed]: Semaphore::close
    Closed,
    /// The semaphore does not currently have enough permits to satisfy the
    /// request.
    InsufficientPermits,
}

/// The semaphore's queue of waiters. This is the portion of the semaphore's
/// state stored inside the lock.
#[derive(Debug)]
struct SemQueue {
    /// The linked list of waiters.
    ///
    /// # Safety
    ///
    /// This is protected by a mutex; the mutex *must* be acquired when
    /// manipulating the linked list, OR when manipulating waiter nodes that may
    /// be linked into the list. If a node is known to not be linked, it is safe
    /// to modify that node (such as by waking the stored [`Waker`]) without
    /// holding the lock; otherwise, it may be modified through the list, so the
    /// lock must be held when modifying the
    /// node.
    queue: List<Waiter>,

    /// Has the semaphore closed?
    ///
    /// This is tracked inside of the locked state to avoid a potential race
    /// condition where the semaphore closes while trying to lock the wait queue.
    closed: bool,
}

#[derive(Debug)]
#[pin_project]
struct Waiter {
    #[pin]
    node: UnsafeCell<Node>,

    remaining_permits: RemainingPermits,
}

/// The number of permits needed before this waiter can be woken.
///
/// When this value reaches zero, the waiter has acquired all its needed
/// permits and can be woken. If this value is `usize::max`, then the waiter
/// has not yet been linked into the semaphore queue.
#[derive(Debug)]
struct RemainingPermits(AtomicUsize);

#[derive(Debug)]
struct Node {
    links: list::Links<Waiter>,
    waker: Option<Waker>,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

// === impl Semaphore ===

impl Semaphore {
    loom_const_fn! {
        /// Returns a new `Semaphore` with `permits` permits available.
        ///
        /// # Panics
        ///
        /// If `permits` is less than [`MAX_PERMITS`] ([`usize::MAX`] - 1).
        ///
        /// [`MAX_PERMITS`]: Self::MAX_PERMITS
        #[must_use]
        pub fn new(permits: usize) -> Self {
            Self::new_with_raw_mutex(permits, Spinlock::new())
        }
    }
}

// This is factored out as a free constant in this module so that `RwLock` can
// depend on it without having to specify `Semaphore`'s type parameters. This is
// a little annoying but whatever.
pub(crate) const MAX_PERMITS: usize = usize::MAX - 1;

impl<Lock: RawMutex> Semaphore<Lock> {
    /// The maximum number of permits a `Semaphore` may contain.
    pub const MAX_PERMITS: usize = MAX_PERMITS;

    const CLOSED: usize = usize::MAX;

    loom_const_fn! {
        /// Returns a new `Semaphore` with `permits` permits available, using the
        /// provided [`RawMutex`] implementation.
        ///
        /// This constructor allows a [`Semaphore`] to be constructed with any type that
        /// implements [`RawMutex`] as the underlying raw blocking mutex
        /// implementation. See [the documentation on overriding mutex
        /// implementations](crate::blocking#overriding-mutex-implementations)
        /// for more details.
        ///
        /// # Panics
        ///
        /// If `permits` is less than [`MAX_PERMITS`] ([`usize::MAX`] - 1).
        ///
        /// [`MAX_PERMITS`]: Self::MAX_PERMITS
        pub fn new_with_raw_mutex(permits: usize, lock: Lock) -> Self {
            assert!(
                permits <= Self::MAX_PERMITS,
                "a semaphore may not have more than Semaphore::MAX_PERMITS permits",
            );
            Self {
                permits: CachePadded::new(AtomicUsize::new(permits)),
                waiters: Mutex::new_with_raw_mutex(SemQueue::new(), lock)
            }
        }
    }

    /// Returns the number of permits currently available in this semaphore, or
    /// 0 if the semaphore is [closed].
    ///
    /// [closed]: Semaphore::close
    pub fn available_permits(&self) -> usize {
        let permits = self.permits.load(Acquire);
        if permits == Self::CLOSED {
            return 0;
        }

        permits
    }

    /// Acquire `permits` permits from the `Semaphore`, waiting asynchronously
    /// if there are insufficient permits currently available.
    ///
    /// # Returns
    ///
    /// - `Ok(`[`Permit`]`)` with the requested number of permits, if the
    ///   permits were acquired.
    /// - `Err(`[`Closed`]`)` if the semaphore was [closed].
    ///
    /// # Cancellation
    ///
    /// This method uses a queue to fairly distribute permits in the order they
    /// were requested. If an [`Acquire`] future is dropped before it completes,
    /// the task will lose its place in the queue.
    ///
    /// [`Closed`]: crate::Closed
    /// [closed]: Semaphore::close
    pub fn acquire(&self, permits: usize) -> Acquire<'_, Lock> {
        Acquire {
            semaphore: self,
            queued: false,
            permits,
            waiter: Waiter::new(permits),
        }
    }

    /// Add `permits` new permits to the semaphore.
    ///
    /// This permanently increases the number of permits available in the
    /// semaphore. The permit count can be permanently *decreased* by calling
    /// [`acquire`] or [`try_acquire`], and [`forget`]ting the returned [`Permit`].
    ///
    /// # Panics
    ///
    /// If adding `permits` permits would cause the permit count to overflow
    /// [`MAX_PERMITS`] ([`usize::MAX`] - 1).
    ///
    /// [`acquire`]: Self::acquire
    /// [`try_acquire`]: Self::try_acquire
    /// [`forget`]: Permit::forget
    /// [`MAX_PERMITS`]: Self::MAX_PERMITS
    #[inline(always)]
    pub fn add_permits(&self, permits: usize) {
        if permits == 0 {
            return;
        }

        self.add_permits_locked(permits, self.waiters.lock());
    }

    /// Try to acquire `permits` permits from the `Semaphore`, without waiting
    /// for additional permits to become available.
    ///
    /// # Returns
    ///
    /// - `Ok(`[`Permit`]`)` with the requested number of permits, if the
    ///   permits were acquired.
    /// - `Err(`[`TryAcquireError::Closed`]`)` if the semaphore was [closed].
    /// - `Err(`[`TryAcquireError::InsufficientPermits`]`)` if the semaphore had
    ///   fewer than `permits` permits available.
    ///
    /// [`Closed`]: crate::Closed
    /// [closed]: Semaphore::close
    pub fn try_acquire(&self, permits: usize) -> Result<Permit<'_, Lock>, TryAcquireError> {
        trace!(permits, "Semaphore::try_acquire");
        self.try_acquire_inner(permits).map(|_| Permit {
            permits,
            semaphore: self,
        })
    }

    /// Closes the semaphore.
    ///
    /// This wakes all tasks currently waiting on the semaphore, and prevents
    /// new permits from being acquired.
    pub fn close(&self) {
        let mut waiters = self.waiters.lock();
        self.permits.store(Self::CLOSED, Release);
        waiters.closed = true;
        while let Some(waiter) = waiters.queue.pop_back() {
            if let Some(waker) = Waiter::take_waker(waiter, &mut waiters.queue) {
                waker.wake();
            }
        }
    }

    fn poll_acquire(
        &self,
        mut node: Pin<&mut Waiter>,
        permits: usize,
        queued: bool,
        cx: &mut Context<'_>,
    ) -> Poll<WaitResult<()>> {
        trace!(
            waiter = ?fmt::ptr(node.as_mut()),
            permits,
            queued,
            "Semaphore::poll_acquire"
        );
        // the total number of permits we've acquired so far.
        let mut acquired_permits = 0;
        let waiter = node.as_mut().project();

        // how many permits are currently needed?
        let needed_permits = if queued {
            waiter.remaining_permits.remaining()
        } else {
            permits
        };

        // okay, let's try to consume the requested number of permits from the
        // semaphore.
        let mut sem_curr = self.permits.load(Relaxed);
        let mut lock = None;
        let mut waiters = loop {
            // semaphore has closed
            if sem_curr == Self::CLOSED {
                return crate::closed();
            }

            // the total number of permits currently available to this waiter
            // are the number it has acquired so far plus all the permits
            // in the semaphore.
            let available_permits = sem_curr + acquired_permits;
            let mut remaining = 0;
            let mut sem_next = sem_curr;
            let can_acquire = if available_permits >= needed_permits {
                // there are enough permits available to satisfy this request.

                // the semaphore's next state will be the current number of
                // permits less the amount we have to take from it to satisfy
                // request.
                sem_next -= needed_permits - acquired_permits;
                needed_permits
            } else {
                // the number of permits available in the semaphore is less than
                // number we want to acquire. take all the currently available
                // permits.
                sem_next = 0;
                // how many permits do we still need to acquire?
                remaining = (needed_permits - acquired_permits) - sem_curr;
                sem_curr
            };

            if remaining > 0 && lock.is_none() {
                // we weren't able to acquire enough permits on this poll, so
                // the waiter will probably need to be queued, so we must lock
                // the wait queue.
                //
                // this has to happen *before* the CAS that sets the new value
                // of the semaphore's permits counter. if we subtracted the
                // permits before acquiring the lock, additional permits might
                // be added to the semaphore while we were waiting to lock the
                // wait queue, and we would miss acquiring those permits.
                // therefore, we lock the queue now.
                lock = Some(self.waiters.lock());
            }

            if let Err(actual) = test_dbg!(self.permits.compare_exchange(
                test_dbg!(sem_curr),
                test_dbg!(sem_next),
                AcqRel,
                Acquire
            )) {
                // the semaphore was updated while we were trying to acquire
                // permits.
                sem_curr = actual;
                continue;
            }

            // okay, we took some permits from the semaphore.
            acquired_permits += can_acquire;
            // did we acquire all the permits we needed?
            if test_dbg!(remaining) == 0 {
                if !queued {
                    // the wasn't already in the queue, so we won't need to
                    // remove it --- we're done!
                    trace!(
                        waiter = ?fmt::ptr(node.as_mut()),
                        permits,
                        queued,
                        "Semaphore::poll_acquire -> all permits acquired; done"
                    );
                    return Poll::Ready(Ok(()));
                } else {
                    // we acquired all the permits we needed, but the waiter was
                    // already in the queue, so we need to dequeue it. we may
                    // have already acquired the lock on a previous CAS attempt
                    // that failed, but if not, grab it now.
                    break lock.unwrap_or_else(|| self.waiters.lock());
                }
            }

            // we updated the semaphore, and will need to wait to acquire
            // additional permits.
            break lock.expect("we should have acquired the lock before trying to wait");
        };

        if waiters.closed {
            trace!(
                waiter = ?fmt::ptr(node.as_mut()),
                permits,
                queued,
                "Semaphore::poll_acquire -> semaphore closed"
            );
            return crate::closed();
        }

        // add permits to the waiter, returning whether we added enough to wake
        // it.
        if waiter.remaining_permits.add(&mut acquired_permits) {
            trace!(
                waiter = ?fmt::ptr(node.as_mut()),
                permits,
                queued,
                "Semaphore::poll_acquire -> remaining permits acquired; done"
            );
            // if there are permits left over after waking the node, give the
            // remaining permits back to the semaphore, potentially assigning
            // them to the next waiter in the queue.
            self.add_permits_locked(acquired_permits, waiters);
            return Poll::Ready(Ok(()));
        }

        debug_assert_eq!(
            acquired_permits, 0,
            "if we are enqueueing a waiter, we must have used all the acquired permits"
        );

        // we need to wait --- register the polling task's waker, and enqueue
        // node.
        let node_ptr = unsafe { NonNull::from(Pin::into_inner_unchecked(node)) };
        Waiter::with_node(node_ptr, &mut waiters.queue, |node| {
            let will_wake = node
                .waker
                .as_ref()
                .map_or(false, |waker| waker.will_wake(cx.waker()));
            if !will_wake {
                node.waker = Some(cx.waker().clone())
            }
        });

        // if the waiter is not already in the queue, add it now.
        if !queued {
            waiters.queue.push_front(node_ptr);
            trace!(
                waiter = ?node_ptr,
                permits,
                queued,
                "Semaphore::poll_acquire -> enqueued"
            );
        }

        Poll::Pending
    }

    #[inline(never)]
    fn add_permits_locked<'sem>(
        &'sem self,
        mut permits: usize,
        mut waiters: MutexGuard<'sem, SemQueue, Lock>,
    ) {
        trace!(permits, "Semaphore::add_permits");
        if waiters.closed {
            trace!(
                permits,
                "Semaphore::add_permits -> already closed; doing nothing"
            );
            return;
        }

        let mut drained_queue = false;
        while permits > 0 && !drained_queue {
            let mut batch = WakeBatch::new();
            while batch.can_add_waker() {
                // peek the last waiter in the queue to add permits to it; we may not
                // be popping it from the queue if there are not enough permits to
                // wake that waiter.
                match waiters.queue.back() {
                    Some(waiter) => {
                        // try to add enough permits to wake this waiter. if we
                        // can't, break --- we should be out of permits.
                        if !waiter.project_ref().remaining_permits.add(&mut permits) {
                            debug_assert_eq!(permits, 0);
                            break;
                        }
                    }
                    None => {
                        // we've emptied the queue. all done!
                        drained_queue = true;
                        break;
                    }
                };

                // okay, we added enough permits to wake this waiter.
                let waiter = waiters
                    .queue
                    .pop_back()
                    .expect("if `back()` returned `Some`, `pop_back()` will also return `Some`");
                let waker = Waiter::take_waker(waiter, &mut waiters.queue);
                trace!(?waiter, ?waker, permits, "Semaphore::add_permits -> waking");
                if let Some(waker) = waker {
                    batch.add_waker(waker);
                }
            }

            if permits > 0 && drained_queue {
                trace!(
                    permits,
                    "Semaphore::add_permits -> queue drained, assigning remaining permits to semaphore"
                );
                // we drained the queue, but there are still permits left --- add
                // them to the semaphore.
                let prev = self.permits.fetch_add(permits, Release);
                assert!(
                    prev + permits <= Self::MAX_PERMITS,
                    "semaphore overflow adding {permits} permits to {prev}; max permits: {}",
                    Self::MAX_PERMITS
                );
            }

            // wake set is full, drop the lock and wake everyone!
            drop(waiters);
            batch.wake_all();

            // reacquire the lock and continue waking
            waiters = self.waiters.lock();
        }
    }

    /// Drop an `Acquire` future.
    ///
    /// This is factored out into a method on `Semaphore`, because the same code
    /// is run when dropping an `Acquire` future or an `AcquireOwned` future.
    fn drop_acquire(&self, waiter: Pin<&mut Waiter>, permits: usize, queued: bool) {
        // If the future is completed, there is no node in the wait list, so we
        // can skip acquiring the lock.
        if !queued {
            return;
        }

        // This is where we ensure safety. The future is being dropped,
        // which means we must ensure that the waiter entry is no longer stored
        // in the linked list.
        let mut waiters = self.waiters.lock();

        let acquired_permits = permits - waiter.remaining_permits.remaining();

        // Safety: we have locked the wait list.
        unsafe {
            // remove the entry from the list
            let node = NonNull::from(Pin::into_inner_unchecked(waiter));
            waiters.queue.remove(node)
        };

        if acquired_permits > 0 {
            self.add_permits_locked(acquired_permits, waiters);
        }
    }

    /// Try to acquire permits from the semaphore without waiting.
    ///
    /// This method is factored out because it's identical between the
    /// `try_acquire` and `try_acquire_owned` methods, which behave identically
    /// but return different permit types.
    fn try_acquire_inner(&self, permits: usize) -> Result<(), TryAcquireError> {
        let mut available = self.permits.load(Relaxed);
        loop {
            // are there enough permits to satisfy the request?
            match available {
                Self::CLOSED => {
                    trace!(permits, "Semaphore::try_acquire -> closed");
                    return Err(TryAcquireError::Closed);
                }
                available if available < permits => {
                    trace!(
                        permits,
                        available,
                        "Semaphore::try_acquire -> insufficient permits"
                    );
                    return Err(TryAcquireError::InsufficientPermits);
                }
                _ => {}
            }

            let remaining = available - permits;
            match self
                .permits
                .compare_exchange_weak(available, remaining, AcqRel, Acquire)
            {
                Ok(_) => {
                    trace!(permits, remaining, "Semaphore::try_acquire -> acquired");
                    return Ok(());
                }
                Err(actual) => available = actual,
            }
        }
    }
}
// === impl SemQueue ===

impl SemQueue {
    #[must_use]
    const fn new() -> Self {
        Self {
            queue: List::new(),
            closed: false,
        }
    }
}

// === impl Acquire ===

impl<'sem, Lock: RawMutex> Future for Acquire<'sem, Lock> {
    type Output = WaitResult<Permit<'sem, Lock>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let poll = this
            .semaphore
            .poll_acquire(this.waiter, *this.permits, *this.queued, cx)
            .map_ok(|_| Permit {
                permits: *this.permits,
                semaphore: this.semaphore,
            });
        *this.queued = poll.is_pending();
        poll
    }
}

#[pinned_drop]
impl<Lock: RawMutex> PinnedDrop for Acquire<'_, Lock> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        trace!(?this.queued, "Acquire::drop");
        this.semaphore
            .drop_acquire(this.waiter, *this.permits, *this.queued)
    }
}

// safety: the `Acquire` future is not automatically `Sync` because the `Waiter`
// node contains an `UnsafeCell`, which is not `Sync`. this impl is safe because
// the `Acquire` future will only access this `UnsafeCell` when mutably borrowed
// (when polling or dropping the future), so the future itself is safe to share
// immutably between threads.
unsafe impl<Lock: RawMutex> Sync for Acquire<'_, Lock> {}

// === impl Permit ===

impl<Lock: RawMutex> Permit<'_, Lock> {
    /// Forget this permit, dropping it *without* returning the number of
    /// acquired permits to the semaphore.
    ///
    /// This permanently decreases the number of permits in the semaphore by
    /// [`self.permits()`](Self::permits).
    pub fn forget(mut self) {
        self.permits = 0;
    }

    /// Returns the count of semaphore permits owned by this `Permit`.
    #[inline]
    #[must_use]
    pub fn permits(&self) -> usize {
        self.permits
    }
}

impl<Lock: RawMutex> Drop for Permit<'_, Lock> {
    fn drop(&mut self) {
        trace!(?self.permits, "Permit::drop");
        self.semaphore.add_permits(self.permits);
    }
}

// === impl TryAcquireError ===

impl fmt::Display for TryAcquireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.pad("semaphore closed"),
            Self::InsufficientPermits => f.pad("semaphore has insufficient permits"),
        }
    }
}

feature! {
    #![feature = "core-error"]
    impl core::error::Error for TryAcquireError {}
}

// === Owned variants when `Arc` is available ===

feature! {
    #![feature = "alloc"]

    use alloc::sync::Arc;

    /// Future returned from [`Semaphore::acquire_owned()`].
    ///
    /// This is identical to the [`Acquire`] future, except that it takes an
    /// [`Arc`] reference to the [`Semaphore`], allowing the returned future to
    /// live for the `'static` lifetime, and returns an [`OwnedPermit`] (rather
    /// than a [`Permit`]), which is also valid for the `'static` lifetime.
    ///
    /// # Notes
    ///
    /// This future is `!Unpin`, as it is unsafe to [`core::mem::forget`] an
    /// `AcquireOwned` future once it has been polled. For instance, the
    /// following code must not compile:
    ///
    ///```compile_fail
    /// use maitake_sync::semaphore::AcquireOwned;
    ///
    /// // Calls to this function should only compile if `T` is `Unpin`.
    /// fn assert_unpin<T: Unpin>() {}
    ///
    /// assert_unpin::<AcquireOwned<'_>>();
    /// ```
    #[derive(Debug)]
    #[pin_project(PinnedDrop)]
    #[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
    pub struct AcquireOwned<Lock: RawMutex = Spinlock> {
        semaphore: Arc<Semaphore<Lock>>,
        queued: bool,
        permits: usize,
        #[pin]
        waiter: Waiter,
    }

    /// An owned [RAII guard] representing one or more permits acquired from a
    /// [`Semaphore`].
    ///
    /// When the `OwnedPermit` is dropped, the permits it represents are
    /// released back to  the [`Semaphore`], potentially waking another task.
    ///
    /// This type is identical to the [`Permit`] type, except that it holds an
    /// [`Arc`] clone of the [`Semaphore`], rather than borrowing it. This
    /// allows the guard to be valid for the `'static` lifetime.
    ///
    /// This type is returned by the [`Semaphore::acquire_owned`] and
    /// [`Semaphore::try_acquire_owned`] methods.
    ///
    /// [RAII guard]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
    #[derive(Debug)]
    #[must_use = "dropping an `OwnedPermit` releases the acquired permits back to the `Semaphore`"]
    pub struct OwnedPermit<Lock: RawMutex = Spinlock>  {
        permits: usize,
        semaphore: Arc<Semaphore<Lock>>,
    }

    impl<Lock: RawMutex> Semaphore<Lock> {
        /// Acquire `permits` permits from the `Semaphore`, waiting asynchronously
        /// if there are insufficient permits currently available, and returning
        /// an [`OwnedPermit`].
        ///
        /// This method behaves identically to [`acquire`], except that it
        /// requires the `Semaphore` to be wrapped in an [`Arc`], and returns an
        /// [`OwnedPermit`] which clones the [`Arc`] rather than borrowing the
        /// semaphore. This allows the returned [`OwnedPermit`] to be valid for
        /// the `'static` lifetime.
        ///
        /// # Returns
        ///
        /// - `Ok(`[`OwnedPermit`]`)` with the requested number of permits, if the
        ///   permits were acquired.
        /// - `Err(`[`Closed`]`)` if the semaphore was [closed].
        ///
        /// # Cancellation
        ///
        /// This method uses a queue to fairly distribute permits in the order they
        /// were requested. If an [`AcquireOwned`] future is dropped before it
        /// completes,   the task will lose its place in the queue.
        ///
        /// [`acquire`]: Semaphore::acquire
        /// [`Closed`]: crate::Closed
        /// [closed]: Semaphore::close
        pub fn acquire_owned(self: &Arc<Self>, permits: usize) -> AcquireOwned<Lock> {
            AcquireOwned {
                semaphore: self.clone(),
                queued: false,
                permits,
                waiter: Waiter::new(permits),
            }
        }

        /// Try to acquire `permits` permits from the `Semaphore`, without waiting
        /// for additional permits to become available, and returning an [`OwnedPermit`].
        ///
        /// This method behaves identically to [`try_acquire`], except that it
        /// requires the `Semaphore` to be wrapped in an [`Arc`], and returns an
        /// [`OwnedPermit`] which clones the [`Arc`] rather than borrowing the
        /// semaphore. This allows the returned [`OwnedPermit`] to be valid for
        /// the `'static` lifetime.
        ///
        /// # Returns
        ///
        /// - `Ok(`[`OwnedPermit`]`)` with the requested number of permits, if the
        ///   permits were acquired.
        /// - `Err(`[`TryAcquireError::Closed`]`)` if the semaphore was [closed].
        /// - `Err(`[`TryAcquireError::InsufficientPermits`]`)` if the semaphore
        ///   had fewer than `permits` permits available.
        ///
        ///
        /// [`try_acquire`]: Semaphore::try_acquire
        /// [`Closed`]: crate::Closed
        /// [closed]: Semaphore::close
        pub fn try_acquire_owned(self: &Arc<Self>, permits: usize) -> Result<OwnedPermit<Lock>, TryAcquireError> {
            trace!(permits, "Semaphore::try_acquire_owned");
            self.try_acquire_inner(permits).map(|_| OwnedPermit {
                permits,
                semaphore: self.clone(),
            })
        }
    }

    // === impl AcquireOwned ===

    impl<Lock: RawMutex> Future for AcquireOwned<Lock> {
        type Output = WaitResult<OwnedPermit<Lock>>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.project();
            let poll = this
                .semaphore
                .poll_acquire(this.waiter, *this.permits, *this.queued, cx)
                .map_ok(|_| OwnedPermit {
                    permits: *this.permits,
                    // TODO(eliza): might be nice to not have to bump the
                    // refcount here...
                    semaphore: this.semaphore.clone(),
                });
            *this.queued = poll.is_pending();
            poll
        }
    }

    #[pinned_drop]
    impl<Lock: RawMutex> PinnedDrop for AcquireOwned<Lock> {
        fn drop(mut self: Pin<&mut Self>) {
            let this = self.project();
            trace!(?this.queued, "AcquireOwned::drop");
            this.semaphore
                .drop_acquire(this.waiter, *this.permits, *this.queued)
        }
    }

    // safety: this is safe for the same reasons as the `Sync` impl for the
    // `Acquire` future.
    unsafe impl<Lock: RawMutex> Sync for AcquireOwned<Lock> {}

    // === impl OwnedPermit ===

    impl<Lock: RawMutex> OwnedPermit<Lock> {
        /// Forget this permit, dropping it *without* returning the number of
        /// acquired permits to the semaphore.
        ///
        /// This permanently decreases the number of permits in the semaphore by
        /// [`self.permits()`](Self::permits).
        pub fn forget(mut self) {
            self.permits = 0;
        }

        /// Returns the count of semaphore permits owned by this `OwnedPermit`.
        #[inline]
        #[must_use]
        pub fn permits(&self) -> usize {
            self.permits
        }
    }

    impl<Lock: RawMutex> Drop for OwnedPermit<Lock> {
        fn drop(&mut self) {
            trace!(?self.permits, "OwnedPermit::drop");
            self.semaphore.add_permits(self.permits);
        }
    }

}

// === impl Waiter ===

impl Waiter {
    fn new(permits: usize) -> Self {
        Self {
            node: UnsafeCell::new(Node {
                links: list::Links::new(),
                waker: None,
                _pin: PhantomPinned,
            }),
            remaining_permits: RemainingPermits(AtomicUsize::new(permits)),
        }
    }

    #[inline(always)]
    #[cfg_attr(loom, track_caller)]
    fn take_waker(this: NonNull<Self>, list: &mut List<Self>) -> Option<Waker> {
        Self::with_node(this, list, |node| node.waker.take())
    }

    /// # Safety
    ///
    /// This is only safe to call while the list is locked. The dummy `_list`
    /// parameter ensures this method is only called while holding the lock, so
    /// this can be safe.
    ///
    /// Of course, that must be the *same* list that this waiter is a member of,
    /// and currently, there is no way to ensure that...
    #[inline(always)]
    #[cfg_attr(loom, track_caller)]
    fn with_node<T>(
        mut this: NonNull<Self>,
        _list: &mut List<Self>,
        f: impl FnOnce(&mut Node) -> T,
    ) -> T {
        unsafe {
            // safety: this is only called while holding the lock on the queue,
            // so it's safe to mutate the waiter.
            this.as_mut().node.with_mut(|node| f(&mut *node))
        }
    }
}

unsafe impl Linked<list::Links<Waiter>> for Waiter {
    type Handle = NonNull<Waiter>;

    fn into_ptr(r: Self::Handle) -> NonNull<Self> {
        r
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

    unsafe fn links(target: NonNull<Self>) -> NonNull<list::Links<Waiter>> {
        // Safety: using `ptr::addr_of!` avoids creating a temporary
        // reference, which stacked borrows dislikes.
        let node = ptr::addr_of!((*target.as_ptr()).node);
        (*node).with_mut(|node| {
            let links = ptr::addr_of_mut!((*node).links);
            // Safety: since the `target` pointer is `NonNull`, we can assume
            // that pointers to its members are also not null, making this use
            // of `new_unchecked` fine.
            NonNull::new_unchecked(links)
        })
    }
}

// === impl RemainingPermits ===

impl RemainingPermits {
    /// Add an acquisition of permits to the waiter, returning whether or not
    /// the waiter has acquired enough permits to be woken.
    #[inline]
    #[cfg_attr(loom, track_caller)]
    fn add(&self, permits: &mut usize) -> bool {
        let mut curr = self.0.load(Relaxed);
        loop {
            let taken = cmp::min(curr, *permits);
            let remaining = curr - taken;
            match self
                .0
                .compare_exchange_weak(curr, remaining, AcqRel, Acquire)
            {
                // added the permits to the waiter!
                Ok(_) => {
                    *permits -= taken;
                    return remaining == 0;
                }
                Err(actual) => curr = actual,
            }
        }
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.0.load(Acquire)
    }
}
