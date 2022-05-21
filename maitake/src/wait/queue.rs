use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{
            AtomicUsize,
            Ordering::{self, *},
        },
    },
    util::{self, tracing},
    wait::{self, WaitResult},
};
use cordyceps::{
    list::{self, List},
    Linked,
};
use core::{
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};
use mycelium_util::{
    fmt,
    sync::{spin::Mutex, CachePadded},
};
use pin_project::{pin_project, pinned_drop};

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

/// Future returned from [`WaitQueue::wait()`].
///
/// This future is fused, so once it has completed, any future calls to poll
/// will immediately return `Poll::Ready`.
#[derive(Debug)]
#[pin_project(PinnedDrop)]
pub struct Wait<'a> {
    /// The `WaitQueue` being received on.
    q: &'a WaitQueue,

    /// The future's state.
    state: State,

    /// Entry in the wait queue.
    #[pin]
    waiter: Waiter,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum State {
    Start,
    Waiting,
    Done(WaitResult),
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

const EMPTY: usize = 0;
const WAITING: usize = 1;
const WAKING: usize = 2;
const CLOSED: usize = 3;

// === impl WaitQueue ===

impl WaitQueue {
    #[inline(always)]
    fn start_wait(&self, node: Pin<&mut Waiter>, cx: &mut Context<'_>) -> Poll<WaitResult> {
        // Optimistically, acquire a stored notification before trying to lock
        // the wait list.
        match test_dbg!(self.state.compare_exchange(WAKING, EMPTY, SeqCst, SeqCst)) {
            Ok(_) => return wait::notified(),
            Err(CLOSED) => return wait::closed(),
            Err(_) => {}
        };

        // Slow path: the queue is not closed, and we failed to consume a stored
        // notification. We need to acquire the lock and enqueue the waiter.
        self.start_wait_slow(node, cx)
    }

    /// Slow path of `start_wait`: acquires the linked list lock, and adds the
    /// waiter to the queue.
    #[cold]
    #[inline(never)]
    fn start_wait_slow(&self, node: Pin<&mut Waiter>, cx: &mut Context<'_>) -> Poll<WaitResult> {
        // There are no queued notifications to consume, and the queue is
        // still open. Therefore, it's time to actually push the waiter to
        // the queue...finally lol :)

        // Grab the lock...
        let mut list = self.list.lock();
        // Reload the queue's state, as it may have changed while we were
        // waiting to lock the linked list.
        let mut state = self.state.load(Acquire);

        loop {
            match test_dbg!(state) {
                // The queue is empty: transition the state to WAITING, as we
                // are adding a waiter.
                EMPTY => {
                    match test_dbg!(self
                        .state
                        .compare_exchange_weak(EMPTY, WAITING, SeqCst, SeqCst))
                    {
                        Ok(_) => break,
                        Err(actual) => {
                            debug_assert!(actual == EMPTY || actual == WAKING || actual == CLOSED);
                            state = actual;
                        }
                    }
                }

                // The queue was woken while we were waiting to acquire the
                // lock. Attempt to consume the wakeup.
                WAKING => {
                    match test_dbg!(self
                        .state
                        .compare_exchange_weak(WAKING, EMPTY, SeqCst, SeqCst))
                    {
                        // Consumed the wakeup!
                        Ok(_) => return wait::notified(),
                        Err(actual) => {
                            debug_assert!(actual == WAKING || actual == EMPTY || actual == CLOSED);
                            state = actual;
                        }
                    }
                }

                // The queue closed while we were waiting to acquire the lock;
                // we're done here!
                CLOSED => return wait::closed(),

                // The queue is already in the WAITING state, so we don't need
                // to mess with it.
                _state => {
                    debug_assert_eq!(_state, WAITING,
                        "start_wait_slow: unexpected state value {:?} (expected WAITING). this is a bug!",
                        _state,
                    );
                    break;
                }
            }
        }

        // Time to wait! Store the waiter in the node, advance the node's state
        // to Waiting, and add it to the queue.

        node.with_node(&mut *list, |node| {
            let _prev = node.waker.replace(cx.waker().clone());
            debug_assert!(
                _prev.is_none(),
                "start_wait_slow: called with a node that already had a waiter!"
            );
        });

        let _prev_state = test_dbg!(node.state.swap(WAITING, Release));
        debug_assert!(
            _prev_state == EMPTY || _prev_state == WAKING,
            "start_wait_slow: called with a node that was not empty ({}) or woken ({})! actual={}",
            EMPTY,
            WAKING,
            _prev_state,
        );
        list.push_front(ptr(node));

        Poll::Pending
    }
}

// === impl Waiter ===

impl Waiter {
    /// # Safety
    ///
    /// This is only safe to call while the list is locked. The dummy `_list`
    /// parameter ensures this method is only called while holding the lock, so
    /// this can be safe.
    #[inline(always)]
    #[cfg_attr(loom, track_caller)]
    fn with_node<T>(&self, _list: &mut List<Self>, f: impl FnOnce(&mut Node) -> T) -> T {
        self.node.with_mut(|node| unsafe {
            // Safety: the dummy `_list` argument ensures that the caller has
            // the right to mutate the list (e.g. the list is locked).
            f(&mut *node)
        })
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

    unsafe fn links(ptr: NonNull<Self>) -> NonNull<list::Links<Waiter>> {
        (*ptr.as_ptr())
            .node
            .with_mut(|node| util::non_null(node).cast::<list::Links<Waiter>>())
    }
}

// === impl Wait ===

impl Future for Wait<'_> {
    type Output = Result<(), wait::Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().project();
        let ptr = ptr(this.waiter);

        tracing::trace!(self = fmt::ptr(ptr), state = ?this.state, "Wait::poll");

        loop {
            let this = self.as_mut().project();

            match *this.state {
                State::Start => match this.q.start_wait(this.waiter, cx) {
                    Poll::Ready(x) => {
                        *this.state = State::Done(x);
                        return Poll::Ready(x);
                    }
                    Poll::Pending => {
                        *this.state = State::Waiting;
                        return Poll::Pending;
                    }
                },
                State::Waiting => todo!(),
                State::Done(res) => return Poll::Ready(res),
            }
        }
    }
}

#[pinned_drop]
impl PinnedDrop for Wait<'_> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        let state = this.state;
        let ptr = ptr(this.waiter);

        tracing::trace!(self = fmt::ptr(ptr), ?state, "Wait::drop");
        if *state == State::Waiting {
            unsafe {
                this.q.list.lock().remove(ptr);
            }
        }
    }
}

fn ptr(pin: Pin<&mut Waiter>) -> NonNull<Waiter> {
    pin.as_ref().get_ref().into()
}
