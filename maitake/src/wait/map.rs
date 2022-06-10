use crate::{
    loom::{
        cell::UnsafeCell,
        sync::{
            atomic::{AtomicUsize, Ordering::*},
            spin::Mutex,
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
    fmt::Debug,
    future::Future,
    marker::PhantomPinned,
    mem,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};
use mycelium_bitfield::FromBits;
use mycelium_util::fmt;
use mycelium_util::sync::{spin::MutexGuard, CachePadded};
use pin_project::{pin_project, pinned_drop};

#[cfg(test)]
mod tests;

/// A queue of [`Waker`]s implemented using an [intrusive doubly-linked
/// list][ilist].
///
/// A `WaitMap` allows any number of tasks to [wait] asynchronously and be
/// woken when some event occurs, either [individually][wake] in first-in,
/// first-out order, or [all at once][wake_all]. This makes it a vital building
/// block of runtime services (such as timers or I/O resources), where it may be
/// used to wake a set of tasks when a timer completes or when a resource
/// becomes available. It can be equally useful for implementing higher-level
/// synchronization primitives: for example, a `WaitMap` plus an
/// [`UnsafeCell`] is essentially an entire implementation of a fair
/// asynchronous mutex. Finally, a `WaitMap` can be a useful synchronization
/// primitive on its own: sometimes, you just need to have a bunch of tasks wait
/// for something and then wake them all up.
///
/// # Examples
///
/// Waking a single task at a time by calling [`wake`][wake]:
///
/// ```skip
/// use std::sync::Arc;
/// use maitake::{scheduler::Scheduler, wait::WaitMap};
///
/// const TASKS: usize = 10;
///
/// // In order to spawn tasks, we need a `Scheduler` instance.
/// let scheduler = Scheduler::new();
///
/// // Construct a new `WaitMap`.
/// let q = Arc::new(WaitMap::new());
///
/// // Spawn some tasks that will wait on the queue.
/// for _ in 0..TASKS {
///     let q = q.clone();
///     scheduler.spawn(async move {
///         // Wait to be woken by the queue.
///         q.wait().await.expect("queue is not closed");
///     });
/// }
///
/// // Tick the scheduler once.
/// let tick = scheduler.tick();
///
/// // No tasks should complete on this tick, as they are all waiting
/// // to be woken by the queue.
/// assert_eq!(tick.completed, 0, "no tasks have been woken");
///
/// let mut completed = 0;
/// for i in 1..=TASKS {
///     // Wake the next task from the queue.
///     q.wake();
///
///     // Tick the scheduler.
///     let tick = scheduler.tick();
///     
///     // A single task should have completed on this tick.
///     completed += tick.completed;
///     assert_eq!(completed, i);
/// }
///
/// assert_eq!(completed, TASKS, "all tasks should have completed");
/// ```
///
/// Waking all tasks using [`wake_all`][wake_all]:
///
/// ```skip
/// use std::sync::Arc;
/// use maitake::{scheduler::Scheduler, wait::WaitMap};
///
/// const TASKS: usize = 10;
///
/// // In order to spawn tasks, we need a `Scheduler` instance.
/// let scheduler = Scheduler::new();
///
/// // Construct a new `WaitMap`.
/// let q = Arc::new(WaitMap::new());
///
/// // Spawn some tasks that will wait on the queue.
/// for _ in 0..TASKS {
///     let q = q.clone();
///     scheduler.spawn(async move {
///         // Wait to be woken by the queue.
///         q.wait().await.expect("queue is not closed");
///     });
/// }
///
/// // Tick the scheduler once.
/// let tick = scheduler.tick();
///
/// // No tasks should complete on this tick, as they are all waiting
/// // to be woken by the queue.
/// assert_eq!(tick.completed, 0, "no tasks have been woken");
///
/// // Wake all tasks waiting for the queue.
/// q.wake_all();
///
/// // Tick the scheduler again to run the woken tasks.
/// let tick = scheduler.tick();
///
/// // All tasks have now completed, since they were woken by the
/// // queue.
/// assert_eq!(tick.completed, TASKS, "all tasks should have completed");
/// ```
///
/// # Implementation Notes
///
/// The *[intrusive]* aspect of this list is important, as it means that it does
/// not allocate memory. Instead, nodes in the linked list are stored in the
/// futures of tasks trying to wait for capacity. This means that it is not
/// necessary to allocate any heap memory for each task waiting to be woken.
///
/// However, the intrusive linked list introduces one new danger: because
/// futures can be *cancelled*, and the linked list nodes live within the
/// futures trying to wait on the queue, we *must* ensure that the node
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
/// [wait]: WaitMap::wait
/// [wake]: WaitMap::wake
/// [wake_all]: WaitMap::wake_all
/// [`UnsafeCell`]: core::cell::UnsafeCell
/// [ilist]: cordyceps::List
/// [intrusive]: https://fuchsia.dev/fuchsia-src/development/languages/c-cpp/fbl_containers_guide/introduction
/// [2]: https://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
pub struct WaitMap<K: PartialEq, V> {
    /// The wait queue's state variable.
    state: CachePadded<AtomicUsize>,

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
    queue: Mutex<List<Waiter<K, V>>>,
}

impl<K: PartialEq, V> Debug for WaitMap<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WaitMap")
            .field("state", &self.state)
            .field("queue", &self.queue)
            .finish()
    }
}
/// Future returned from [`WaitMap::wait()`].
///
/// This future is fused, so once it has completed, any future calls to poll
/// will immediately return [`Poll::Ready`].
#[derive(Debug)]
#[pin_project(PinnedDrop)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Wait<'a, K: PartialEq, V> {
    /// The [`WaitMap`] being waited on from.
    queue: &'a WaitMap<K, V>,

    /// Entry in the wait queue linked list.
    #[pin]
    waiter: Waiter<K, V>,
}

/// A waiter node which may be linked into a wait queue.
#[repr(C)]
#[pin_project]
struct Waiter<K: PartialEq, V> {
    /// The intrusive linked list node.
    ///
    /// This *must* be the first field in the struct in order for the `Linked`
    /// implementation to be sound.
    #[pin]
    node: UnsafeCell<Node<K, V>>,

    /// The future's state.
    state: WaitState,

    key: K,
    val: UnsafeCell<Option<V>>,
}

impl<K: PartialEq, V> Debug for Waiter<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Waiter")
            .field("node", &self.node)
            .field("state", &self.state)
            .field("key", &fmt::display(core::any::type_name::<K>()))
            .field("val", &fmt::display(core::any::type_name::<V>()))
            .finish()
    }
}

#[repr(C)]
struct Node<K: PartialEq, V> {
    /// Intrusive linked list pointers.
    ///
    /// # Safety
    ///
    /// This *must* be the first field in the struct in order for the `Linked`
    /// impl to be sound.
    links: list::Links<Waiter<K, V>>,

    /// The node's waker
    waker: Wakeup,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

impl<K: PartialEq, V> Debug for Node<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Node")
            .field("links", &self.links)
            .field("waker", &self.waker)
            .finish()
    }
}

/// The state of a [`Waiter`] node in a [`WaitMap`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
enum WaitState {
    /// The waiter has not yet been enqueued.
    ///
    /// The number of times [`WaitMap::wake_all`] has been called is stored
    /// when the node is created, in order to determine whether it was woken by
    /// a stored wakeup when enqueueing.
    ///
    /// When in this state, the node is **not** part of the linked list, and
    /// can be dropped without removing it from the list.
    Start,

    /// The waiter is waiting.
    ///
    /// When in this state, the node **is** part of the linked list. If the
    /// node is dropped in this state, it **must** be removed from the list
    /// before dropping it. Failure to ensure this will result in dangling
    /// pointers in the linked list!
    Waiting,

    /// The waiter has been woken.
    ///
    /// When in this state, the node is **not** part of the linked list, and
    /// can be dropped without removing it from the list.
    Woken,
}

/// The queue's current state.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
enum State {
    /// No waiters are queued, and there is no pending notification.
    /// Waiting while the queue is in this state will enqueue the waiter;
    /// notifying while in this state will store a pending notification in the
    /// queue, transitioning to [`State::Woken`].
    Empty = 0b00,

    /// There are one or more waiters in the queue. Waiting while
    /// the queue is in this state will not transition the state. Waking while
    /// in this state will wake the first waiter in the queue; if this empties
    /// the queue, then the queue will transition to [`State::Empty`].
    Waiting = 0b01,

    /// The queue has a stored notification. Waiting while the queue
    /// is in this state will consume the pending notification *without*
    /// enqueueing the waiter and transition the queue to [`State::Empty`].
    /// Waking while in this state will leave the queue in this state.
    Woken = 0b10,

    /// The queue is closed. Waiting while in this state will return
    /// [`Closed`] without transitioning the queue's state.
    ///
    /// *Note*: This *must* correspond to all state bits being set, as it's set
    /// via a [`fetch_or`].
    ///
    /// [`Closed`]: crate::wait::Closed
    /// [`fetch_or`]: core::sync::atomic::AtomicUsize::fetch_or
    Closed = 0b11,
}

#[derive(Clone, Debug)]
enum Wakeup {
    Empty,
    Waiting(Waker),
    One,
    Closed,
}

// === impl WaitMap ===

impl<K: PartialEq, V> WaitMap<K, V> {
    /// Returns a new `WaitMap`.
    #[must_use]
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            state: CachePadded::new(AtomicUsize::new(State::Empty.into_usize())),
            queue: Mutex::new(List::new()),
        }
    }

    /// Returns a new `WaitMap`.
    #[must_use]
    #[cfg(loom)]
    pub fn new() -> Self {
        Self {
            state: CachePadded::new(AtomicUsize::new(State::Empty.into_usize())),
            queue: Mutex::new(List::new()),
        }
    }

    /// Wake a certain task in the queue.
    ///
    /// If the queue is empty, a wakeup is stored in the `WaitMap`, and the
    /// next call to [`wait`] will complete immediately.
    ///
    /// [`wait`]: WaitMap::wait
    #[inline]
    pub fn wake(&self, key: &K, val: V) -> WakeOutcome<V> {
        // snapshot the queue's current state.
        let mut state = self.load();

        // check if any tasks are currently waiting on this queue. if there are
        // no waiting tasks, store the wakeup to be consumed by the next call to
        // `wait`.
        loop {
            match state {
                // if the queue is closed, bail.
                State::Closed => return WakeOutcome::NoMatch(val),
                // if there are waiting tasks, break out of the loop and wake one.
                State::Waiting => break,
                _ => {}
            }

            let next = State::Woken;
            // advance the state to `Woken`, and return (if we did so
            // successfully)
            match self.compare_exchange(state, next) {
                Ok(_) => return WakeOutcome::NoMatch(val),
                Err(actual) => state = actual,
            }
        }

        // okay, there are tasks waiting on the queue; we must acquire the lock
        // on the linked list and wake the next task from the queue.
        let mut queue = self.queue.lock();
        test_trace!("wake: -> locked");

        // the queue's state may have changed while we were waiting to acquire
        // the lock, so we need to acquire a new snapshot.
        state = self.load();

        if let Some(mut node) = self.node_match_locked(key, &mut *queue, state) {
            let waker = Waiter::<K, V>::wake(node, &mut *queue, Wakeup::One);
            drop(queue);
            unsafe {
                let mut new = UnsafeCell::new(Some(val));
                core::mem::swap(&mut new, &mut node.as_mut().val);
                #[cfg(debug_assertions)]
                new.with(|v| {
                    debug_assert!((*v).is_none());
                });
            }
            waker.wake();
            WakeOutcome::Woke
        } else {
            WakeOutcome::NoMatch(val)
        }
    }

    /// Close the queue, indicating that it may no longer be used.
    ///
    /// Once a queue is closed, all [`wait`] calls (current or future) will
    /// return an error.
    ///
    /// This method is generally used when implementing higher-level
    /// synchronization primitives or resources: when an event makes a resource
    /// permanently unavailable, the queue can be closed.
    pub fn close(&self) {
        let state = self.state.fetch_or(State::Closed.into_usize(), SeqCst);
        let state = test_dbg!(State::from_bits(state));
        if state != State::Waiting {
            return;
        }

        let mut queue = self.queue.lock();

        // TODO(eliza): wake outside the lock using an array, a la
        // https://github.com/tokio-rs/tokio/blob/4941fbf7c43566a8f491c64af5a4cd627c99e5a6/tokio/src/sync/batch_semaphore.rs#L277-L303
        while let Some(node) = queue.pop_back() {
            let waker = Waiter::wake(node, &mut queue, Wakeup::Closed);
            waker.wake()
        }
    }

    /// Wait to be woken up by this queue.
    ///
    /// This returns a [`Wait`] future that will complete when the task is
    /// woken by a call to [`wake`] or [`wake_all`], or when the `WaitMap` is
    /// dropped.
    ///
    /// [`wake`]: Self::wake
    /// [`wake_all`]: Self::wake_all
    pub fn wait(&self, key: K) -> Wait<'_, K, V> {
        self.try_wait(key)
            .expect("Keys should be unique in a `WaitMap`!")
    }

    pub fn try_wait(&self, key: K) -> Result<Wait<'_, K, V>, ()> {
        // Check if key already exists
        let mut mg = self.queue.lock();
        let mut cursor = mg.cursor();
        if cursor.find(|n| n.key == key).is_some() {
            return Err(());
        }
        drop(mg);

        Ok(Wait {
            queue: self,
            waiter: self.waiter(key),
        })
    }

    /// Returns a [`Waiter`] entry in this queue.
    ///
    /// This is factored out into a separate function because it's used by both
    /// [`WaitMap::wait`] and [`WaitMap::wait_owned`].
    fn waiter(&self, key: K) -> Waiter<K, V> {
        // how many times has `wake_all` been called when this waiter is created?
        let state = WaitState::Start;
        Waiter {
            state,
            node: UnsafeCell::new(Node {
                links: list::Links::new(),
                waker: Wakeup::Empty,
                _pin: PhantomPinned,
            }),
            key,
            val: UnsafeCell::new(None),
        }
    }

    #[cfg_attr(test, track_caller)]
    fn load(&self) -> State {
        #[allow(clippy::let_and_return)]
        let state = State::from_bits(self.state.load(SeqCst));
        test_trace!("state.load() = {state:?}");
        state
    }

    #[cfg_attr(test, track_caller)]
    fn store(&self, state: State) {
        test_trace!("state.store({state:?}");
        self.state.store(state as usize, SeqCst);
    }

    #[cfg_attr(test, track_caller)]
    fn compare_exchange(&self, current: State, new: State) -> Result<State, State> {
        #[allow(clippy::let_and_return)]
        let res = self
            .state
            .compare_exchange(current as usize, new as usize, SeqCst, SeqCst)
            .map(State::from_bits)
            .map_err(State::from_bits);
        test_trace!("state.compare_exchange({current:?}, {new:?}) = {res:?}");
        res
    }

    #[cold]
    #[inline(never)]
    fn node_match_locked(
        &self,
        key: &K,
        queue: &mut List<Waiter<K, V>>,
        curr: State,
    ) -> Option<NonNull<Waiter<K, V>>> {
        let state = curr;

        // is the queue still in the `Waiting` state? it is possible that we
        // transitioned to a different state while locking the queue.
        if test_dbg!(state) != State::Waiting {
            // if there are no longer any queued tasks, try to store the
            // wakeup in the queue and bail.
            if let Err(actual) = self.compare_exchange(curr, State::Woken) {
                debug_assert!(actual != State::Waiting);
                self.store(State::Woken);
            }

            return None;
        }

        let mut cursor = queue.cursor();
        let opt_node = cursor.remove_first(|t| &t.key == key);

        // if we took the final waiter currently in the queue, transition to the
        // `Empty` state.
        if test_dbg!(queue.is_empty()) {
            self.store(State::Empty);
        }

        opt_node
    }

    #[cold]
    #[inline(never)]
    fn wake_next_locked(&self, queue: &mut List<Waiter<K, V>>, curr: State) -> Option<Waker> {
        let state = curr;

        // is the queue still in the `Waiting` state? it is possible that we
        // transitioned to a different state while locking the queue.
        if test_dbg!(state) != State::Waiting {
            // if there are no longer any queued tasks, try to store the
            // wakeup in the queue and bail.
            if let Err(actual) = self.compare_exchange(curr, State::Woken) {
                debug_assert!(actual != State::Waiting);
                self.store(State::Woken);
            }

            return None;
        }

        // otherwise, we have to dequeue a task and wake it.
        let node = queue
            .pop_back()
            .expect("if we are in the Waiting state, there must be waiters in the queue");
        let waker = Waiter::<K, V>::wake(node, queue, Wakeup::One);

        // if we took the final waiter currently in the queue, transition to the
        // `Empty` state.
        if test_dbg!(queue.is_empty()) {
            self.store(State::Empty);
        }

        Some(waker)
    }
}

pub enum WakeOutcome<V> {
    Woke,
    NoMatch(V),
}

// === impl Waiter ===

impl<K: PartialEq, V> Waiter<K, V> {
    /// Wake the task that owns this `Waiter`.
    ///
    /// # Safety
    ///
    /// This is only safe to call while the list is locked. The `list`
    /// parameter ensures this method is only called while holding the lock, so
    /// this can be safe.
    ///
    /// Of course, that must be the *same* list that this waiter is a member of,
    /// and currently, there is no way to ensure that...
    #[inline(always)]
    #[cfg_attr(loom, track_caller)]
    fn wake(this: NonNull<Self>, list: &mut List<Self>, wakeup: Wakeup) -> Waker {
        Waiter::with_node(this, list, |node| {
            let waker = test_dbg!(mem::replace(&mut node.waker, wakeup));
            match waker {
                Wakeup::Waiting(waker) => waker,
                _ => unreachable!("tried to wake a waiter in the {:?} state!", waker),
            }
        })
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
        f: impl FnOnce(&mut Node<K, V>) -> T,
    ) -> T {
        unsafe {
            // safety: this is only called while holding the lock on the queue,
            // so it's safe to mutate the waiter.
            this.as_mut().node.with_mut(|node| f(&mut *node))
        }
    }

    fn poll_wait(
        mut self: Pin<&mut Self>,
        queue: &WaitMap<K, V>,
        cx: &mut Context<'_>,
    ) -> Poll<WaitResult<Option<V>>> {
        test_trace!(ptr = ?fmt::ptr(self.as_mut()), "Waiter::poll_wait");
        let mut this = self.as_mut().project();

        match test_dbg!(&this.state) {
            WaitState::Start => {
                // can we consume a pending wakeup?
                if queue.compare_exchange(State::Woken, State::Empty).is_ok() {
                    *this.state = WaitState::Woken;
                    let val = this.val.with_mut(|v| unsafe { (*v).take() });

                    return Poll::Ready(Ok(val));
                }

                // okay, no pending wakeups. try to wait...
                test_trace!("poll_wait: locking...");
                let mut waiters = queue.queue.lock();
                test_trace!("poll_wait: -> locked");
                let mut queue_state = queue.load();

                // transition the queue to the waiting state
                'to_waiting: loop {
                    match test_dbg!(queue_state) {
                        // the queue is `Empty`, transition to `Waiting`
                        State::Empty => match queue.compare_exchange(queue_state, State::Waiting) {
                            Ok(_) => break 'to_waiting,
                            Err(actual) => queue_state = actual,
                        },
                        // the queue is already `Waiting`
                        State::Waiting => break 'to_waiting,
                        // the queue was woken, consume the wakeup.
                        State::Woken => match queue.compare_exchange(queue_state, State::Empty) {
                            Ok(_) => {
                                *this.state = WaitState::Woken;
                                let val = this.val.with_mut(|v| unsafe { (*v).take() });

                                return Poll::Ready(Ok(val));
                            }
                            Err(actual) => queue_state = actual,
                        },
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
                let ptr = unsafe { NonNull::from(Pin::into_inner_unchecked(self)) };
                waiters.push_front(ptr);

                Poll::Pending
            }
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
                        }
                        Wakeup::One => {
                            *this.state = WaitState::Woken;
                            let val = this.val.with_mut(|v| (*v).take());

                            Poll::Ready(Ok(val))
                        }
                        Wakeup::Closed => {
                            *this.state = WaitState::Woken;
                            wait::closed()
                        }
                        Wakeup::Empty => unreachable!(),
                    }
                })
            }
            WaitState::Woken => {
                let val = this.val.with_mut(|v| unsafe { (*v).take() });

                Poll::Ready(Ok(val))
            }
        }
    }

    /// Release this `Waiter` from the queue.
    ///
    /// This is called from the `drop` implementation for the [`Wait`] and
    /// [`WaitOwned`] futures.
    fn release(mut self: Pin<&mut Self>, queue: &WaitMap<K, V>) {
        let state = *(self.as_mut().project().state);
        let ptr = NonNull::from(unsafe { Pin::into_inner_unchecked(self) });
        test_trace!(self = ?fmt::ptr(ptr), ?state, ?queue, "Waiter::release");

        // if we're not enqueued, we don't have to do anything else.
        if state != WaitState::Waiting {
            return;
        }

        let mut waiters: MutexGuard<List<Waiter<K, V>>> = queue.queue.lock();
        let state = queue.load();

        // remove the node
        unsafe {
            // safety: we have the lock on the queue, so this is safe.
            waiters.remove(ptr);
        };

        // if we removed the last waiter from the queue, transition the state to
        // `Empty`.
        if test_dbg!(waiters.is_empty()) && state == State::Waiting {
            queue.store(State::Empty);
        }

        // if the node has an unconsumed wakeup, it must be assigned to the next
        // node in the queue.
        if Waiter::<K, V>::with_node(ptr, &mut waiters, |node| matches!(&node.waker, Wakeup::One)) {
            if let Some(waker) = queue.wake_next_locked(&mut waiters, state) {
                drop(waiters);
                waker.wake()
            }
        }
    }
}

unsafe impl<K: PartialEq, V> Linked<list::Links<Waiter<K, V>>> for Waiter<K, V> {
    type Handle = NonNull<Waiter<K, V>>;

    fn into_ptr(r: Self::Handle) -> NonNull<Self> {
        r
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

    unsafe fn links(ptr: NonNull<Self>) -> NonNull<list::Links<Waiter<K, V>>> {
        (*ptr.as_ptr())
            .node
            .with_mut(|node| util::non_null(node).cast::<list::Links<Waiter<K, V>>>())
    }
}

// === impl Wait ===

impl<K: PartialEq, V> Future for Wait<'_, K, V> {
    type Output = WaitResult<Option<V>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.waiter.poll_wait(this.queue, cx)
    }
}

#[pinned_drop]
impl<K: PartialEq, V> PinnedDrop for Wait<'_, K, V> {
    fn drop(mut self: Pin<&mut Self>) {
        let this = self.project();
        this.waiter.release(this.queue);
    }
}

// === impl MapState ===

impl State {
    fn from_bits(bits: usize) -> Self {
        match bits as u8 {
            bits if bits == Self::Empty as u8 => Self::Empty,
            bits if bits == Self::Waiting as u8 => Self::Waiting,
            bits if bits == Self::Woken as u8 => Self::Woken,
            bits if bits == Self::Closed as u8 => Self::Closed,
            _ => unsafe {
                mycelium_util::unreachable_unchecked!(
                    "all potential 2-bit patterns should be covered!"
                )
            },
        }
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
            _ => unsafe {
                mycelium_util::unreachable_unchecked!(
                    "all potential 2-bit patterns should be covered!"
                )
            },
        })
    }

    fn into_bits(self) -> usize {
        self.into_usize()
    }
}

impl State {
    const fn into_usize(self) -> usize {
        self as u8 as usize
    }
}

// === impl WaitState ===

impl FromBits<usize> for WaitState {
    const BITS: u32 = 2;
    type Error = &'static str;

    fn try_from_bits(bits: usize) -> Result<Self, Self::Error> {
        match bits as u8 {
            bits if bits == Self::Start as u8 => Ok(Self::Start),
            bits if bits == Self::Waiting as u8 => Ok(Self::Waiting),
            bits if bits == Self::Woken as u8 => Ok(Self::Woken),
            _ => Err("invalid `WaitState`; expected one of Start, Waiting, or Woken"),
        }
    }

    fn into_bits(self) -> usize {
        self as u8 as usize
    }
}

// === impl WaitOwned ===

feature! {
    #![feature = "alloc"]

    use alloc::sync::Arc;

    /// Future returned from [`WaitMap::wait_owned()`].
    ///
    /// This is identical to the [`Wait`] future, except that it takes an
    /// [`Arc`] reference to the [`WaitMap`], allowing the returned future to
    /// live for the `'static` lifetime.
    ///
    /// This future is fused, so once it has completed, any future calls to poll
    /// will immediately return [`Poll::Ready`].
    #[derive(Debug)]
    #[pin_project(PinnedDrop)]
    pub struct WaitOwned<K: PartialEq, V> {
        /// The `WaitMap` being waited on.
        queue: Arc<WaitMap<K, V>>,

        /// Entry in the wait queue.
        #[pin]
        waiter: Waiter<K, V>,
    }

    impl<K: PartialEq, V> WaitMap<K, V> {
        /// Wait to be woken up by this queue, returning a future that's valid
        /// for the `'static` lifetime.
        ///
        /// This returns a [`WaitOwned`] future that will complete when the task is
        /// woken by a call to [`wake`] or [`wake_all`], or when the `WaitMap` is
        /// dropped.
        ///
        /// This is identical to the [`wait`] method, except that it takes a
        /// [`Arc`] reference to the [`WaitMap`], allowing the returned future to
        /// live for the `'static` lifetime.
        ///
        /// [`wake`]: Self::wake
        /// [`wake_all`]: Self::wake_all
        /// [`wait`]: Self::wait
        pub fn wait_owned(self: &Arc<Self>, key: K) -> WaitOwned<K, V> {
            let waiter = self.waiter(key);
            let queue = self.clone();
            WaitOwned { queue, waiter }
        }
    }

    impl<K: PartialEq, V> Future for WaitOwned<K, V> {
        type Output = WaitResult<Option<V>>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.project();
            this.waiter.poll_wait(&*this.queue, cx)
        }
    }

    #[pinned_drop]
    impl<K: PartialEq, V> PinnedDrop for WaitOwned<K, V> {
        fn drop(mut self: Pin<&mut Self>) {
            let this = self.project();
            this.waiter.release(&*this.queue);
        }
    }
}
