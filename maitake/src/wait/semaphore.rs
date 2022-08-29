use crate::{
    loom::{
        cell::UnsafeCell,
        sync::{
            atomic::{AtomicUsize, Ordering::*},
            spin::{Mutex, MutexGuard},
        },
    },
    wait::{self, WaitResult},
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
#[cfg(any(test, feature = "tracing-01", feature = "tracing-02"))]
use mycelium_util::fmt;
use mycelium_util::sync::CachePadded;
use pin_project::{pin_project, pinned_drop};

#[cfg(all(test, loom))]
mod loom;
#[cfg(all(test, not(loom)))]
mod tests;

#[derive(Debug)]
pub struct Semaphore {
    /// The number of permits in the semaphore (or a flag indicating that it is closed).
    permits: CachePadded<AtomicUsize>,
    /// The queue of tasks waiting to acquire permits.
    ///
    /// A spinlock (from `mycelium_util`) is used here, in order to support
    /// `no_std` platforms; when running `loom` tests, a `loom` mutex is used
    /// instead to simulate the spinlock, because loom doesn't play nice with
    /// real spinlocks.
    waiters: Mutex<SemQueue>,
}

#[derive(Debug)]
pub struct Permit<'sem> {
    permits: usize,
    semaphore: &'sem Semaphore,
}

#[derive(Debug)]
#[pin_project(PinnedDrop)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Acquire<'sem> {
    semaphore: &'sem Semaphore,
    queued: bool,
    permits: usize,
    #[pin]
    waiter: Waiter,
}

/// Errors returned by [`Semaphore::try_acquire`].

#[derive(Debug, PartialEq)]
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
    pub const MAX_PERMITS: usize = usize::MAX >> 2;
    const CLOSED: usize = usize::MAX;

    loom_const_fn! {
        #[must_use]
        pub fn new(permits: usize) -> Self {
            assert!(
                permits <= Self::MAX_PERMITS,
                "a semaphore may not have more than Semaphore::MAX_PERMITS permits",
        );
            Self {
                permits: CachePadded::new(AtomicUsize::new(permits)),
                waiters: Mutex::new(SemQueue {
                    queue: List::new(),
                    closed: false,
                }),
            }
        }
    }

    pub fn available_permits(&self) -> usize {
        let permits = self.permits.load(Acquire);
        if permits == Self::CLOSED {
            return 0;
        }

        permits
    }

    pub fn acquire(&self, permits: usize) -> Acquire<'_> {
        Acquire {
            semaphore: self,
            queued: false,
            permits,
            waiter: Waiter {
                node: UnsafeCell::new(Node {
                    links: list::Links::new(),
                    waker: None,
                    _pin: PhantomPinned,
                }),
                remaining_permits: RemainingPermits(AtomicUsize::new(permits)),
            },
        }
    }

    #[inline(always)]
    pub fn add_permits(&self, permits: usize) {
        if permits == 0 {
            return;
        }

        self.add_permits_locked(permits, self.waiters.lock());
    }

    pub fn try_acquire(&self, permits: usize) -> Result<Permit<'_>, TryAcquireError> {
        trace!(permits, "Semaphore::try_acquire");
        self.try_acquire_inner(permits).map(|_| Permit {
            permits,
            semaphore: self,
        })
    }

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
                return wait::closed();
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
            return wait::closed();
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
    fn add_permits_locked(&self, mut permits: usize, mut waiters: MutexGuard<'_, SemQueue>) {
        trace!(permits, "Semaphore::add_permits");
        if waiters.closed {
            trace!(
                permits,
                "Semaphore::add_permits -> already closed; doing nothing"
            );
            return;
        }

        let mut drained_queue = false;
        while permits > 0 {
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
                // TODO(eliza): wake in batches outside the lock.
                waker.wake();
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

// === impl Acquire ===

impl<'sem> Future for Acquire<'sem> {
    type Output = WaitResult<Permit<'sem>>;
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
impl PinnedDrop for Acquire<'_> {
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
unsafe impl Sync for Acquire<'_> {}

// === impl Permit ===

impl Permit<'_> {
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

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        trace!(?self.permits, "Permit::drop");
        self.semaphore.add_permits(self.permits);
    }
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
    #[derive(Debug)]
    #[pin_project(PinnedDrop)]
    #[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
    pub struct AcquireOwned {
        semaphore: Arc<Semaphore>,
        queued: bool,
        permits: usize,
        #[pin]
        waiter: Waiter,
    }


    #[derive(Debug)]
    pub struct OwnedPermit {
        permits: usize,
        semaphore: Arc<Semaphore>,
    }

    impl Semaphore {
        pub fn acquire_owned(self: &Arc<Self>, permits: usize) -> AcquireOwned {
            AcquireOwned {
                semaphore: self.clone(),
                queued: false,
                permits,
                waiter: Waiter::new(permits),
            }
        }

        pub fn try_acquire_owned(self: &Arc<Self>, permits: usize) -> Result<OwnedPermit, TryAcquireError> {
            trace!(permits, "Semaphore::try_acquire_owned");
            self.try_acquire_inner(permits).map(|_| OwnedPermit {
                permits,
                semaphore: self.clone(),
            })
        }
    }

    // === impl AcquireOwned ===

    impl Future for AcquireOwned {
        type Output = WaitResult<OwnedPermit>;

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
    impl PinnedDrop for AcquireOwned {
        fn drop(mut self: Pin<&mut Self>) {
            let this = self.project();
            trace!(?this.queued, "AcquireOwned::drop");
            this.semaphore
                .drop_acquire(this.waiter, *this.permits, *this.queued)
        }
    }

    // safety: this is safe for the same reasons as the `Sync` impl for the
    // `Acquire` future.
    unsafe impl Sync for AcquireOwned {}

    // === impl OwnedPermit ===

    impl OwnedPermit {
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

    impl Drop for OwnedPermit {
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
