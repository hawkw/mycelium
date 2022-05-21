use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{
            AtomicUsize,
            Ordering::{self, *},
        },
    },
    util,
};
use cordyceps::{
    list::{self, List},
    Linked,
};
use core::{marker::PhantomPinned, ptr::NonNull, task::Waker};
use mycelium_util::sync::{spin::Mutex, CachePadded};

/// A queue of [`Waker`]s implemented using an [intrusive singly-linked list][ilist].
///
/// The *[intrusive]* aspect of this list is important, as it means that it does
/// not allocate memory. Instead, nodes in the linked list are stored in the
/// futures of tasks trying to wait for capacity. This means that it is not
/// necessary to allocate any heap memory for each task waiting to be notified.
///
/// However, the intrusive linked list introduces one new danger: because
/// futures can be *cancelled*, and the linked list nodes live within the
/// futures trying to wait for channel capacity, we *must* ensure that the node
/// is unlinked from the list before dropping a cancelled future. Failure to do
/// so would result in the list containing dangling pointers. Therefore, we must
/// use a *doubly-linked* list, so that nodes can edit both the previous and
/// next node when they have to remove themselves. This is kind of a bummer, as
/// it means we can't use something nice like this [intrusive queue by Dmitry
/// Vyukov][2], and there are not really practical designs for lock-free
/// doubly-linked lists that don't rely on some kind of deferred reclamation
/// scheme such as hazard pointers or QSBR.
///
/// Instead, we just stick a [`Mutex`] around the linked list, which must be
/// acquired to pop nodes from it, or for nodes to remove themselves when
/// futures are cancelled. This is a bit sad, but the critical sections for this
/// mutex are short enough that we still get pretty good performance despite it.
///
/// [`Waker`]: core::task::Waker
/// [ilist]: cordyceps::List
/// [intrusive]: https://fuchsia.dev/fuchsia-src/development/languages/c-cpp/fbl_containers_guide/introduction
/// [2]: https://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
#[derive(Debug)]
pub struct WaitQueue {
    /// The wait queue's state variable.
    ///
    /// The queue is always in one of the following states:
    ///
    /// - [`EMPTY`]: No waiters are queued, and there is no pending notification.
    ///   Waiting while the queue is in this state will enqueue the waiter;
    ///   notifying while in this state will store a pending notification in the
    ///   queue, transitioning to the `WAKING` state.
    ///
    /// - [`WAITING`]: There are one or more waiters in the queue. Waiting while
    ///   the queue is in this state will not transition the state. Waking while
    ///   in this state will wake the first waiter in the queue; if this empties
    ///   the queue, then the queue will transition to the `EMPTY` state.
    ///
    /// - [`WAKING`]: The queue has a stored notification. Waiting while the queue
    ///   is in this state will consume the pending notification *without*
    ///   enqueueing the waiter and transition the queue to the `EMPTY` state.
    ///   Waking while in this state will leave the queue in this state.
    ///
    /// - [`CLOSED`]: The queue is closed. Waiting while in this state will return
    ///   [`WaitResult::Closed`] without transitioning the queue's state.
    state: CachePadded<AtomicUsize>,

    /// The linked list of waiters.
    ///
    /// # Safety
    ///
    /// This is protected by a mutex; the mutex *must* be acquired when
    /// manipulating the linked list, OR when manipulating waiter nodes that may
    /// be linked into the list. If a node is known to not be linked, it is safe
    /// to modify that node (such as by setting or unsetting its
    /// `Waker`/`Thread`) without holding the lock; otherwise, it may be
    /// modified through the list, so the lock must be held when modifying the
    /// node.
    ///
    /// A spinlock is used on `no_std` platforms; [`std::sync::Mutex`] or
    /// `parking_lot::Mutex` are used when the standard library is available
    /// (depending on feature flags).
    list: Mutex<List<Waiter>>,
}

/// A waiter node which may be linked into a wait queue.
#[derive(Debug)]
#[repr(C)]
struct Waiter {
    /// The intrusive linked list node.
    ///
    /// This *must* be the first field in the struct in order for the `Linked`
    /// implementation to be sound.
    node: UnsafeCell<Node>,

    /// The waiter's state variable.
    ///
    /// A waiter is always in one of the following states:
    ///
    /// - [`EMPTY`]: The waiter is not linked in the queue, and does not have a
    ///   `Thread`/`Waker`.
    ///
    /// - [`WAITING`]: The waiter is linked in the queue and has a
    ///   `Thread`/`Waker`.
    ///
    /// - [`WAKING`]: The waiter has been notified by the wait queue. If it is in
    ///   this state, it is *not* linked into the queue, and does not have a
    ///   `Thread`/`Waker`.
    ///
    /// - [`WAKING`]: The waiter has been notified because the wait queue closed.
    ///   If it is in this state, it is *not* linked into the queue, and does
    ///   not have a `Thread`/`Waker`.
    ///
    /// This may be inspected without holding the lock; it can be used to
    /// determine whether the lock must be acquired.
    state: CachePadded<AtomicUsize>,
}

#[derive(Debug)]
#[repr(C)]
struct Node {
    /// Intrusive linked list pointers.
    ///
    /// # Safety
    ///
    /// This *must* be the first field in the struct in order for the `Linked`
    /// impl to be sound.
    links: list::Links<Waiter>,

    /// The node's waker
    waker: Option<Waker>,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

unsafe impl Linked<list::Links<Waiter>> for Waiter {
    type Handle = NonNull<Waiter>;

    fn into_ptr(r: Self::Handle) -> NonNull<Self> {
        r
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

    unsafe fn links(ptr: NonNull<Self>) -> NonNull<list::Links<Waiter>> {
        (*ptr.as_ptr())
            .node
            .with_mut(|node| util::non_null(node).cast::<list::Links<Waiter>>())
    }
}
