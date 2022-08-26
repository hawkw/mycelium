use crate::loom::{
    cell::UnsafeCell,
    sync::{
        atomic::{AtomicUsize, Ordering::*},
        spin::{Mutex, MutexGuard},
    },
};
use crate::wait::WaitResult;
use cordyceps::{
    list::{self, List},
    Linked,
};
use core::{
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    ptr::{self, NonNull},
    task::{Context, Poll, Waker},
};
use mycelium_util::sync::CachePadded;
use pin_project::{pin_project, pinned_drop};

#[derive(Debug)]
pub struct Semaphore {
    /// The number of permits in the semaphore (or a flag indicating that it is closed).
    permits: CachePadded<AtomicUsize>,

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
    ///
    /// A spinlock (from `mycelium_util`) is used here, in order to support
    /// `no_std` platforms; when running `loom` tests, a `loom` mutex is used
    /// instead to simulate the spinlock, because loom doesn't play nice with
    /// real spinlocks.
    queue: Mutex<List<Waiter>>,
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

#[derive(Debug)]
pub struct AcquireError(());

#[derive(Debug)]
#[pin_project]
struct Waiter {
    #[pin]
    node: UnsafeCell<Node>,

    /// The number of permits needed before this waiter can be woken.
    ///
    /// When this value reaches zero, the waiter has acquired all its needed
    /// permits and can be woken. If this value is `usize::max`, then the waiter
    /// has not yet been linked into the semaphore queue.
    remaining_permits: AtomicUsize,
}

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
                queue: Mutex::new(List::new()),
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
                remaining_permits: AtomicUsize::new(permits),
            },
        }
    }

    fn poll_acquire(
        &self,
        node: Pin<&mut Waiter>,
        num_permits: usize,
        queued: bool,
        cx: &mut Context<'_>,
    ) -> Poll<WaitResult<()>> {
        todo!("eliza")
    }

    fn add_permits_locked(&self, permits: usize, waiters: MutexGuard<'_, List<Waiter>>) {
        todo!("eliza")
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
        *this.queued = poll.is_ready();
        poll
    }
}

#[pinned_drop]
impl PinnedDrop for Acquire<'_> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        // If the future is completed, there is no node in the wait list, so we
        // can skip acquiring the lock.
        if !*this.queued {
            return;
        }

        // This is where we ensure safety. The future is being dropped,
        // which means we must ensure that the waiter entry is no longer stored
        // in the linked list.
        let mut queue = this.semaphore.queue.lock();

        let acquired_permits = *this.permits - this.waiter.remaining_permits.load(Acquire);

        // Safety: we have locked the wait list.
        unsafe {
            // remove the entry from the list
            let node = NonNull::from(Pin::into_inner_unchecked(this.waiter));
            queue.remove(node)
        };

        if acquired_permits > 0 {
            this.semaphore.add_permits_locked(acquired_permits, queue);
        }
    }
}

// === impl Waiter ===

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
