use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{
            AtomicUsize,
            Ordering::{self, *},
        },
    },
    util,
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
    mem,
};
use mycelium_bitfield::{FromBits, bitfield};
use mycelium_util::{
    fmt,
    sync::{spin::Mutex, CachePadded},
};
use pin_project::{pin_project, pinned_drop};

#[cfg(test)]
mod tests;

/// A queue of [`Waker`]s implemented using an [intrusive doubly-linked list][ilist].
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
    queue: Mutex<List<Waiter>>,
}

/// Future returned from [`WaitQueue::wait()`].
///
/// This future is fused, so once it has completed, any future calls to poll
/// will immediately return `Poll::Ready`.
#[derive(Debug)]
#[pin_project(PinnedDrop)]
pub struct Wait<'a> {
    /// The `WaitQueue` being waited on from.
    queue: &'a WaitQueue,

    /// Entry in the wait queue.
    #[pin]
    waiter: Waiter,
}


#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum WaitState {
    Start(usize),
    Waiting,
    Woken,
}

/// A waiter node which may be linked into a wait queue.
#[derive(Debug)]
#[repr(C)]
#[pin_project]
struct Waiter {
    /// The intrusive linked list node.
    ///
    /// This *must* be the first field in the struct in order for the `Linked`
    /// implementation to be sound.
    #[pin]
    node: UnsafeCell<Node>,

    /// The future's state.
    state: WaitState,
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
    waker: Wakeup,

    // /// Optional user data.
    // data: Option<T>,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

bitfield! {
    #[derive(Eq, PartialEq)]
    struct QueueState<usize> {
        const STATE: State;
        const WAKE_ALLS = ..;
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
enum State {
    /// No waiters are queued, and there is no pending notification.
    /// Waiting while the queue is in this state will enqueue the waiter;
    /// notifying while in this state will store a pending notification in the
    /// queue, transitioning to the [`Woken`] state.
    Empty = 0,

    /// There are one or more waiters in the queue. Waiting while
    /// the queue is in this state will not transition the state. Waking while
    /// in this state will wake the first waiter in the queue; if this empties
    /// the queue, then the queue will transition to the [`Empty`] state.
    Waiting = 1,

    /// The queue has a stored notification. Waiting while the queue
    /// is in this state will consume the pending notification *without*
    /// enqueueing the waiter and transition the queue to the [`Empty`] state.
    /// Waking while in this state will leave the queue in this state.
    Woken = 2,

    /// The queue is closed. Waiting while in this state will return
    /// [`Closed`] without transitioning the queue's state.
    Closed = 3,

}

impl QueueState {
    const ONE_WAKE_ALL: usize = Self::WAKE_ALLS.first_bit();

    fn with_state(self, state: State) -> Self {
        self.with(Self::STATE, state)
    }
}

impl FromBits<usize> for State {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: usize) -> Result<Self, Self::Error> {
        Ok(match bits as u8 {
            bits if bits == Self::Empty as u8 => Self::Empty,
            bits if bits == Self::Waiting as u8 => Self::Waiting,
            bits if bits == Self::Woken as u8 => Self::Woken,
            bits if bits == Self::Closed as u8 => Self::Closed,
            _ => unsafe { mycelium_util::unreachable_unchecked!("all potential 2-bit patterns should be covered!") },
        })
    }

    fn into_bits(self) -> usize {
        self as u8 as usize
    }
}

#[derive(Clone, Debug)]
enum Wakeup {
    Empty,
    Waiting(Waker),
    One,
    All,
    Closed,
}


// === impl WaitQueue ===

impl WaitQueue {

    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            state: CachePadded::new(AtomicUsize::new(0)),
            queue: Mutex::new(List::new()),
        }
    }

    #[cfg(loom)]
    pub fn new() -> Self {
        Self {
            state: CachePadded::new(AtomicUsize::new(0)),
            queue: Mutex::new(List::new()),
        }
    }


    pub fn wake(&self) {
        let mut state = self.load();
        while state.get(QueueState::STATE) != State::Waiting {
            let next = state.with_state(State::Woken);
            match self.compare_exchange(state, next) {
                Ok(_) => return,
                Err(actual) => state = actual,
            }
        }

        let mut queue = self.queue.lock();

        test_trace!("wake: -> locked");
        state = self.load();

        if let Some(waker) = self.wake_locked(&mut *queue, state) {
            waker.wake();
        }
    }

    pub fn wake_all(&self) {
        let mut queue = self.queue.lock();
        let state = self.load();

        // if there are no waiters in the queue, increment the number of
        // `wake_all` calls and return.
        if state.get(QueueState::STATE) != State::Waiting {
            self.state.fetch_add(QueueState::ONE_WAKE_ALL, SeqCst);
            return;
        }

        // okay, we actually have to wake some stuff.

        // TODO(eliza): wake outside the lock using an array, a la
        // https://github.com/tokio-rs/tokio/blob/4941fbf7c43566a8f491c64af5a4cd627c99e5a6/tokio/src/sync/batch_semaphore.rs#L277-L303
        while let Some(node) = queue.pop_back() {
            let waker = Waiter::wake(node, &mut queue, Wakeup::All);
            waker.wake()
        }

        // now that the queue has been drained, transition to the empty state,
        // and increment the wake_all count.
        let next_state = QueueState::new().with_state(State::Empty).with(QueueState::WAKE_ALLS, state.get(QueueState::WAKE_ALLS) + 1);
        self.compare_exchange(state, next_state).expect("state should not have transitioned while locked");
    }


    pub fn wait(&self) -> Wait<'_> {
        let current_wake_alls = test_dbg!(self.load().get(QueueState::WAKE_ALLS));
        Wait {
            queue: self,
            waiter: Waiter {
                state: WaitState::Start(current_wake_alls),
                node: UnsafeCell::new(Node {
                    links: list::Links::new(),
                    waker: Wakeup::Empty,
                    _pin: PhantomPinned,
                }),
            },
        }
    }


    #[cfg_attr(test, track_caller)]
    fn load(&self) -> QueueState {
        #[allow(clippy::let_and_return)]
        let state = QueueState::from_bits(self.state.load(SeqCst));
        test_trace!("state.load() = {state:?}");
        state
    }

    #[cfg_attr(test, track_caller)]
    fn store(&self, state: QueueState) {
        test_trace!("state.store({state:?}");
        self.state.store(state.0, SeqCst);
    }

    #[cfg_attr(test, track_caller)]
    fn compare_exchange(&self, current: QueueState, new: QueueState) -> Result<QueueState, QueueState> {
        #[allow(clippy::let_and_return)]
        let res = self.state.compare_exchange(current.0, new.0, SeqCst, SeqCst).map(QueueState::from_bits).map_err(QueueState::from_bits);
        test_trace!("state.compare_exchange({current:?}, {new:?}) = {res:?}");
        res
    }

    #[cold]
    #[inline(never)]
    fn wake_locked(&self, queue: &mut List<Waiter>, curr: QueueState) -> Option<Waker> {
        let state = curr.get(QueueState::STATE);
        if test_dbg!(state) != State::Waiting {

            if let Err(actual) = self.compare_exchange(curr, curr.with_state(State::Waiting)) {
                debug_assert!(actual.get(QueueState::STATE) != State::Waiting);
                self.store(actual.with_state(State::Woken));
            }

            return None;
        }


        let node = queue.pop_back()
            .expect("if we are in the Waiting state, there must be waiters in the queue");
        let waker = Waiter::wake(node, queue, Wakeup::One);

        if test_dbg!(queue.is_empty()) {
            // we have taken the final waiter from the queue
            self.store(curr.with_state(State::Empty));
        }

        Some(waker)
    }
}

impl Drop for WaitQueue {
    fn drop(&mut self) {
        let mut queue = self.queue.lock();
        test_dbg!(self.state.fetch_or(State::Closed as u8 as usize, SeqCst));

        // TODO(eliza): wake outside the lock using an array, a la
        // https://github.com/tokio-rs/tokio/blob/4941fbf7c43566a8f491c64af5a4cd627c99e5a6/tokio/src/sync/batch_semaphore.rs#L277-L303
        while let Some(node) = queue.pop_back() {
            let waker = Waiter::wake(node, &mut queue, Wakeup::Closed);
            waker.wake()
        }
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
    fn wake(mut this: NonNull<Self>, _list: &mut List<Self>, wakeup: Wakeup) -> Waker {
        unsafe {
            // safety: this is only called while holding the lock on the queue,
            // so it's safe to mutate the waiter.
            this.as_mut().node.with_mut(|node| {
                let waker = test_dbg!(mem::replace(&mut (*node).waker, wakeup));
                match waker {
                    Wakeup::Waiting(waker) => waker,
                    _ => unreachable!("tried to wake a waiter in the {:?} state!", waker),
                }
            })
        }
    }

    fn poll_wait(mut self: Pin<&mut Self>, queue: &WaitQueue, cx: &mut Context<'_>) -> Poll<WaitResult> {
        test_trace!(ptr = ?fmt::ptr(self.as_mut()), "Waiter::poll_wait");
        let mut this = self.as_mut().project();

        match test_dbg!(*this.state) {
            WaitState::Start(wake_alls) => {
                let mut queue_state = queue.load();

                // can we consume a pending wakeup?
                if queue.compare_exchange(queue_state.with_state(State::Woken), queue_state.with_state(State::Empty)).is_ok() {
                    *this.state = WaitState::Woken;
                    return Poll::Ready(Ok(()));
                }

                // okay, no pending wakeups. try to wait...
                test_trace!("poll_wait: locking...");
                let mut waiters = queue.queue.lock();
                test_trace!("poll_wait: -> locked");
                queue_state = test_dbg!(queue.load());

                // the whole queue was woken while we were trying to acquire
                // the lock!
                if queue_state.get(QueueState::WAKE_ALLS) != wake_alls {
                    *this.state = WaitState::Woken;
                    return Poll::Ready(Ok(()));
                }

                // transition the queue to the waiting state
                'to_waiting: loop {
                    match test_dbg!(queue_state.get(QueueState::STATE)) {
                        // the queue is EMPTY, transition to WAITING
                        State::Empty => {
                            match queue.compare_exchange(queue_state, queue_state.with_state(State::Waiting)) {
                                Ok(_) => break 'to_waiting,
                                Err(actual) => queue_state = actual,
                            }
                        },
                        // the queue is already WAITING
                        State::Waiting => break 'to_waiting,
                        // the queue was woken, consume the wakeup.
                        State::Woken => {
                            match queue.compare_exchange(queue_state, queue_state.with_state(State::Empty)) {
                                Ok(_) => {
                                    *this.state = WaitState::Woken;
                                    return Poll::Ready(Ok(()));
                                }
                                Err(actual) => queue_state = actual,
                            }
                        }
                        State::Closed => return wait::closed(),
                    }
                }

                // enqueue the node
                *this.state = WaitState::Waiting;
                this.node.as_mut().with_mut(|node| {
                    unsafe {
                        // safety: we may mutate the node because we are
                        // holding the lock.
                        (*node).waker = Wakeup::Waiting(cx.waker().clone());
                    }
                });
                let ptr = unsafe {
                    NonNull::from(Pin::into_inner_unchecked(self))
                };
                waiters.push_front(ptr);

                Poll::Pending
            },
            WaitState::Waiting => {
                let mut _waiters = queue.queue.lock();
                this.node.with_mut(|node| unsafe {
                    // safety: we may mutate the node because we are
                    // holding the lock.
                    let node = &mut *node;
                    match node.waker {
                        Wakeup::Waiting(ref mut waker) => {
                            if !waker.will_wake(cx.waker()) {
                                *waker = cx.waker().clone();
                            }
                            Poll::Pending
                        },
                        Wakeup::All | Wakeup::One => {
                            *this.state = WaitState::Woken;
                            Poll::Ready(Ok(()))
                        },
                        Wakeup::Closed => {
                            *this.state = WaitState::Woken;
                            wait::closed()
                        }
                        Wakeup::Empty => unreachable!(),
                    }
                })
            },
            WaitState::Woken => Poll::Ready(Ok(())),
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

    unsafe fn links(ptr: NonNull<Self>) -> NonNull<list::Links<Waiter>> {
        (*ptr.as_ptr())
            .node
            .with_mut(|node| util::non_null(node).cast::<list::Links<Waiter>>())
    }
}

// === impl Wait ===

impl Future for Wait<'_> {
    type Output = WaitResult;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.waiter.poll_wait(this.queue, cx)
    }
}

#[pinned_drop]
impl PinnedDrop for Wait<'_> {
    fn drop(mut self: Pin<&mut Self>) {
        let mut this = self.as_mut().project();
        let state = *(this.waiter.as_mut().project().state);
        let ptr = NonNull::from(unsafe {
            Pin::into_inner_unchecked(this.waiter)
        });
        test_trace!(self = ?fmt::ptr(ptr), ?state, "Wait::drop");
        if state == WaitState::Waiting {
            unsafe {
                this.queue.queue.lock().remove(ptr);
            }
        }
    }
}