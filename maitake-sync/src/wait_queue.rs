//! A queue of waiting tasks that can be woken in first-in, first-out order (or
//! all at once).
//!
//! See the [`WaitQueue`] type's documentation for details.
#[cfg(any(test, maitake_ultraverbose))]
use crate::util::fmt;
use crate::{
    blocking::{DefaultMutex, Mutex, ScopedRawMutex},
    loom::{
        cell::UnsafeCell,
        sync::atomic::{AtomicUsize, Ordering::*},
    },
    util::{CachePadded, WakeBatch},
    WaitResult,
};
use cordyceps::{
    list::{self, List},
    Linked,
};
use core::{
    future::Future,
    marker::PhantomPinned,
    mem,
    pin::Pin,
    ptr::{self, NonNull},
    task::{Context, Poll, Waker},
};
use mycelium_bitfield::{bitfield, enum_from_bits, FromBits};
use pin_project::{pin_project, pinned_drop};

#[cfg(test)]
mod tests;

/// A queue of waiting tasks which can be [woken in first-in, first-out
/// order][wake], or [all at once][wake_all].
///
/// A `WaitQueue` allows any number of tasks to [wait] asynchronously and be
/// woken when some event occurs, either [individually][wake] in first-in,
/// first-out order, or [all at once][wake_all]. This makes it a vital building
/// block of runtime services (such as timers or I/O resources), where it may be
/// used to wake a set of tasks when a timer completes or when a resource
/// becomes available. It can be equally useful for implementing higher-level
/// synchronization primitives: for example, a `WaitQueue` plus an
/// [`UnsafeCell`] is essentially [an entire implementation of a fair
/// asynchronous mutex][mutex]. Finally, a `WaitQueue` can be a useful
/// synchronization primitive on its own: sometimes, you just need to have a
/// bunch of tasks wait for something and then wake them all up.
///
/// # Overriding the blocking mutex
///
/// This type uses a [blocking `Mutex`](crate::blocking::Mutex) internally to
/// synchronize access to its wait list. By default, this is a [`DefaultMutex`]. To
/// use an alternative [`ScopedRawMutex`] implementation, use the
/// [`new_with_raw_mutex`](Self::new_with_raw_mutex) constructor. See [the documentation
/// on overriding mutex
/// implementations](crate::blocking#overriding-mutex-implementations) for more
/// details.
///
/// # Examples
///
/// Waking a single task at a time by calling [`wake`][wake]:
///
/// ```ignore
/// use std::sync::Arc;
/// use maitake::scheduler::Scheduler;
/// use maitake_sync::WaitQueue;
///
/// const TASKS: usize = 10;
///
/// // In order to spawn tasks, we need a `Scheduler` instance.
/// let scheduler = Scheduler::new();
///
/// // Construct a new `WaitQueue`.
/// let q = Arc::new(WaitQueue::new());
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
/// ```ignore
/// use std::sync::Arc;
/// use maitake::scheduler::Scheduler;
/// use maitake_sync::WaitQueue;
///
/// const TASKS: usize = 10;
///
/// // In order to spawn tasks, we need a `Scheduler` instance.
/// let scheduler = Scheduler::new();
///
/// // Construct a new `WaitQueue`.
/// let q = Arc::new(WaitQueue::new());
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
/// This type is implemented using an [intrusive doubly-linked list][ilist].
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
/// [wait]: WaitQueue::wait
/// [wake]: WaitQueue::wake
/// [wake_all]: WaitQueue::wake_all
/// [`UnsafeCell`]: core::cell::UnsafeCell
/// [ilist]: cordyceps::List
/// [intrusive]: https://fuchsia.dev/fuchsia-src/development/languages/c-cpp/fbl_containers_guide/introduction
/// [mutex]: crate::Mutex
/// [2]: https://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
#[derive(Debug)]
pub struct WaitQueue<Lock: ScopedRawMutex = DefaultMutex> {
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
    /// A spinlock is used here, in order to support `no_std` platforms; when
    /// running `loom` tests, a `loom` mutex is used instead to simulate the
    /// spinlock, because loom doesn't play nice with  real spinlocks.
    queue: Mutex<List<Waiter>, Lock>,
}

/// Future returned from [`WaitQueue::wait()`].
///
/// This future is fused, so once it has completed, any future calls to poll
/// will immediately return [`Poll::Ready`].
///
/// # Notes
///
/// This future is `!Unpin`, as it is unsafe to [`core::mem::forget`] a `Wait`
/// future once it has been polled. For instance, the following code must not
/// compile:
///
///```compile_fail
/// use maitake_sync::wait_queue::Wait;
///
/// // Calls to this function should only compile if `T` is `Unpin`.
/// fn assert_unpin<T: Unpin>() {}
///
/// assert_unpin::<Wait<'_>>();
/// ```
#[derive(Debug)]
#[pin_project(PinnedDrop)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Wait<'a, Lock: ScopedRawMutex = DefaultMutex> {
    /// The [`WaitQueue`] being waited on.
    queue: &'a WaitQueue<Lock>,

    /// Entry in the wait queue linked list.
    #[pin]
    waiter: Waiter,
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
    state: WaitStateBits,
}

#[derive(Debug)]
struct Node {
    /// Intrusive linked list pointers.
    links: list::Links<Waiter>,

    /// The node's waker
    waker: Wakeup,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

bitfield! {
    #[derive(Eq, PartialEq)]
    struct QueueState<usize> {
        /// The queue's state.
        const STATE: State;

        /// The number of times [`WaitQueue::wake_all`] has been called.
        const WAKE_ALLS = ..;
    }
}

bitfield! {
    #[derive(Eq, PartialEq)]
    struct WaitStateBits<usize> {
        /// The waiter's state.
        const STATE: WaitState;

        /// The number of times [`WaitQueue::wake_all`] has been called.
        const WAKE_ALLS = ..;
    }
}

enum_from_bits! {
    /// The state of a [`Waiter`] node in a [`WaitQueue`].
    #[derive(Debug, Eq, PartialEq)]
    enum WaitState<u8> {
        /// The waiter has not yet been enqueued.
        ///
        /// The number of times [`WaitQueue::wake_all`] has been called is stored
        /// when the node is created, in order to determine whether it was woken by
        /// a stored wakeup when enqueueing.
        ///
        /// When in this state, the node is **not** part of the linked list, and
        /// can be dropped without removing it from the list.
        Start = 0b00,

        /// The waiter is waiting.
        ///
        /// When in this state, the node **is** part of the linked list. If the
        /// node is dropped in this state, it **must** be removed from the list
        /// before dropping it. Failure to ensure this will result in dangling
        /// pointers in the linked list!
        Waiting = 0b01,

        /// The waiter has been woken.
        ///
        /// When in this state, the node is **not** part of the linked list, and
        /// can be dropped without removing it from the list.
        Woken = 0b10,
    }
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
    /// [`Closed`]: crate::Closed
    /// [`fetch_or`]: core::sync::atomic::AtomicUsize::fetch_or
    Closed = 0b11,
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
    loom_const_fn! {
        /// Returns a new `WaitQueue`.
        ///
        /// This constructor returns a `WaitQueue` that uses a [`DefaultMutex`] as
        /// the [`ScopedRawMutex`] implementation for wait list synchronization.
        /// To use a different [`ScopedRawMutex`] implementation, use the
        /// [`new_with_raw_mutex`](Self::new_with_raw_mutex) constructor, instead. See
        /// [the documentation on overriding mutex
        /// implementations](crate::blocking#overriding-mutex-implementations)
        /// for more details.
        #[must_use]
        pub fn new() -> Self {
            Self::new_with_raw_mutex(DefaultMutex::new())
        }
    }
}

impl<Lock> WaitQueue<Lock>
where
    Lock: ScopedRawMutex,
{
    loom_const_fn! {
        /// Returns a new `WaitQueue`, using the provided [`ScopedRawMutex`]
        /// implementation for wait-list synchronization.
        ///
        /// This constructor allows a `WaitQueue` to be constructed with any type that
        /// implements [`ScopedRawMutex`] as the underlying raw blocking mutex
        /// implementation. See [the documentation on overriding mutex
        /// implementations](crate::blocking#overriding-mutex-implementations)
        /// for more details.
        #[must_use]
        pub fn new_with_raw_mutex(lock: Lock) -> Self {
            Self::make(State::Empty, lock)
        }
    }

    loom_const_fn! {
        /// Returns a new `WaitQueue` with a single stored wakeup.
        ///
        /// The first call to [`wait`] on this queue will immediately succeed.
        ///
        /// [`wait`]: Self::wait
        // TODO(eliza): should this be a public API?
        #[must_use]
        pub(crate) fn new_woken(lock: Lock) -> Self {
            Self::make(State::Woken, lock)
        }
    }

    loom_const_fn! {
        #[must_use]
        fn make(state: State, lock: Lock) -> Self {
            Self {
                state: CachePadded::new(AtomicUsize::new(state.into_usize())),
                queue: Mutex::new_with_raw_mutex(List::new(), lock),
            }
        }
    }

    /// Wake the next task in the queue.
    ///
    /// If the queue is empty, a wakeup is stored in the `WaitQueue`, and the
    /// **next** call to [`wait().await`] will complete immediately. If one or more
    /// tasks are currently in the queue, the first task in the queue is woken.
    ///
    /// At most one wakeup will be stored in the queue at any time. If `wake()`
    /// is called many times while there are no tasks in the queue, only a
    /// single wakeup is stored.
    ///
    /// [`wait().await`]: Self::wait()
    ///
    /// # Examples
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use std::sync::Arc;
    /// use maitake_sync::WaitQueue;
    ///
    /// let queue = Arc::new(WaitQueue::new());
    ///
    /// let waiter = task::spawn({
    ///     // clone the queue to move into the spawned task
    ///     let queue = queue.clone();
    ///     async move {
    ///         queue.wait().await;
    ///         println!("received wakeup!");
    ///     }
    /// });
    ///
    /// println!("waking task...");
    /// queue.wake();
    ///
    /// waiter.await.unwrap();
    /// # }
    /// # test();
    /// ```
    #[inline]
    pub fn wake(&self) {
        // snapshot the queue's current state.
        let mut state = self.load();

        // check if any tasks are currently waiting on this queue. if there are
        // no waiting tasks, store the wakeup to be consumed by the next call to
        // `wait`.
        loop {
            match state.get(QueueState::STATE) {
                // if the queue is closed, bail.
                State::Closed => return,
                // if there are waiting tasks, break out of the loop and wake one.
                State::Waiting => break,
                _ => {}
            }

            let next = state.with_state(State::Woken);
            // advance the state to `Woken`, and return (if we did so
            // successfully)
            match self.compare_exchange(state, next) {
                Ok(_) => return,
                Err(actual) => state = actual,
            }
        }

        // okay, there are tasks waiting on the queue; we must acquire the lock
        // on the linked list and wake the next task from the queue.
        let waker = self.queue.with_lock(|queue| {
            test_debug!("wake: -> locked");

            // the queue's state may have changed while we were waiting to acquire
            // the lock, so we need to acquire a new snapshot before we take the
            // waker.
            state = self.load();
            self.wake_locked(queue, state)
        });

        //now that we've released the lock, wake the waiting task (if we
        //actually deuqueued one).
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Wake *all* tasks currently in the queue.
    ///
    /// All tasks currently waiting on the queue are woken. Unlike [`wake()`], a
    /// wakeup is *not* stored in the queue to wake the next call to [`wait()`]
    /// if the queue is empty. Instead, this method only wakes all currently
    /// registered waiters. Registering a task to be woken is done by `await`ing
    /// the [`Future`] returned by the [`wait()`] method on this queue.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use maitake_sync::WaitQueue;
    /// use std::sync::Arc;
    ///
    /// let queue = Arc::new(WaitQueue::new());
    ///
    /// // spawn multiple tasks to wait on the queue.
    /// let task1 = task::spawn({
    ///     let queue = queue.clone();
    ///     async move {
    ///         println!("task 1 waiting...");
    ///         queue.wait().await;
    ///         println!("task 1 woken")
    ///     }
    /// });
    ///
    /// let task2 = task::spawn({
    ///     let queue = queue.clone();
    ///     async move {
    ///         println!("task 2 waiting...");
    ///         queue.wait().await;
    ///         println!("task 2 woken")
    ///     }
    /// });
    ///
    /// // yield to the scheduler so that both tasks register
    /// // themselves to wait on the queue.
    /// task::yield_now().await;
    ///
    /// // neither task will have been woken.
    /// assert!(!task1.is_finished());
    /// assert!(!task2.is_finished());
    ///
    /// // wake all tasks waiting on the queue.
    /// queue.wake_all();
    ///
    /// // yield to the scheduler again so that the tasks can execute.
    /// task::yield_now().await;
    ///
    /// assert!(task1.is_finished());
    /// assert!(task2.is_finished());
    /// # }
    /// # test();
    /// ```
    ///
    /// [`wake()`]: Self::wake
    /// [`wait()`]: Self::wait
    pub fn wake_all(&self) {
        let mut batch = WakeBatch::new();
        let mut waiters_remaining = true;

        // This is a little bit contorted: we must load the state inside the
        // lock, but for all states except for `Waiting`, we just need to bail
        // out...but we can't `return` from the outer function inside the lock
        // closure. Therefore, we just return a `bool` and, if it's `true`,
        // return instead of doing more work.
        let done = self.queue.with_lock(|queue| {
            let state = self.load();

            match test_dbg!(state.get(QueueState::STATE)) {
                // If there are no waiters in the queue, increment the number of
                // `wake_all` calls and return. incrementing the `wake_all` count
                // must be performed inside the lock, so we do it here.
                State::Woken | State::Empty => {
                    self.state.fetch_add(QueueState::ONE_WAKE_ALL, SeqCst);
                    true
                }
                // If the queue is already closed, this is a no-op. Just bail.
                State::Closed => true,
                // Okay, there are waiters in the queue. Transition to the empty
                // state inside the lock and start draining the queue.
                State::Waiting => {
                    let next_state = QueueState::new()
                        .with_state(State::Empty)
                        .with(QueueState::WAKE_ALLS, state.get(QueueState::WAKE_ALLS) + 1);
                    self.compare_exchange(state, next_state)
                        .expect("state should not have transitioned while locked");

                    // Drain the first batch of waiters from the queue.
                    waiters_remaining =
                        test_dbg!(Self::drain_to_wake_batch(&mut batch, queue, Wakeup::All));

                    false
                }
            }
        });

        if done {
            return;
        }

        batch.wake_all();
        // As long as there are waiters remaining to wake, lock the queue, drain
        // another batch, release the lock, and wake them.
        while waiters_remaining {
            self.queue.with_lock(|queue| {
                waiters_remaining = Self::drain_to_wake_batch(&mut batch, queue, Wakeup::All);
            });
            batch.wake_all();
        }
    }

    /// Close the queue, indicating that it may no longer be used.
    ///
    /// Once a queue is closed, all [`wait()`] calls (current or future) will
    /// return an error.
    ///
    /// This method is generally used when implementing higher-level
    /// synchronization primitives or resources: when an event makes a resource
    /// permanently unavailable, the queue can be closed.
    ///
    /// [`wait()`]: Self::wait
    pub fn close(&self) {
        let state = self.state.fetch_or(State::Closed.into_usize(), SeqCst);
        let state = test_dbg!(QueueState::from_bits(state));
        if state.get(QueueState::STATE) != State::Waiting {
            return;
        }

        let mut batch = WakeBatch::new();
        let mut waking = true;
        while waking {
            waking = self
                .queue
                .with_lock(|queue| Self::drain_to_wake_batch(&mut batch, queue, Wakeup::Closed));
            batch.wake_all();
        }
    }

    /// Wait to be woken up by this queue.
    ///
    /// Equivalent to:
    ///
    /// ```ignore
    /// async fn wait(&self);
    /// ```
    ///
    /// This returns a [`Wait`] [`Future`] that will complete when the task is
    /// woken by a call to [`wake()`] or [`wake_all()`], or when the `WaitQueue`
    /// is dropped.
    ///
    /// Each `WaitQueue` holds a single wakeup. If [`wake()`] was previously
    /// called while no tasks were waiting on the queue, then `wait().await`
    /// will complete immediately, consuming the stored wakeup. Otherwise,
    /// `wait().await` waits to be woken by the next call to [`wake()`] or
    /// [`wake_all()`].
    ///
    /// The [`Wait`] future is not guaranteed to receive wakeups from calls to
    /// [`wake()`] if it has not yet been polled. See the documentation for the
    /// [`Wait::subscribe()`] method for details on receiving wakeups from the
    /// queue prior to polling the `Wait` future for the first time.
    ///
    /// A `Wait` future **is** is guaranteed to recieve wakeups from calls to
    /// [`wake_all()`] as soon as it is created, even if it has not yet been
    /// polled.
    ///
    /// # Returns
    ///
    /// The [`Future`] returned by this method completes with one of the
    /// following [outputs](Future::Output):
    ///
    /// - [`Ok`]`(())` if the task was woken by a call to [`wake()`] or
    ///   [`wake_all()`].
    /// - [`Err`]`(`[`Closed`]`)` if the task was woken by the `WaitQueue` being
    ///   [`close`d](WaitQueue::close).
    ///
    /// # Cancellation
    ///
    /// A `WaitQueue` fairly distributes wakeups to waiting tasks in the order
    /// that they started to wait. If a [`Wait`] future is dropped, the task
    /// will forfeit its position in the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use std::sync::Arc;
    /// use maitake_sync::WaitQueue;
    ///
    /// let queue = Arc::new(WaitQueue::new());
    ///
    /// let waiter = task::spawn({
    ///     // clone the queue to move into the spawned task
    ///     let queue = queue.clone();
    ///     async move {
    ///         queue.wait().await;
    ///         println!("received wakeup!");
    ///     }
    /// });
    ///
    /// println!("waking task...");
    /// queue.wake();
    ///
    /// waiter.await.unwrap();
    /// # }
    /// # test();
    /// ```
    ///
    /// [`wake()`]: Self::wake
    /// [`wake_all()`]: Self::wake_all
    /// [`Closed`]: crate::Closed
    pub fn wait(&self) -> Wait<'_, Lock> {
        Wait {
            queue: self,
            waiter: self.waiter(),
        }
    }

    pub(crate) fn try_wait(&self) -> Poll<WaitResult<()>> {
        let mut state = self.load();
        let initial_wake_alls = state.get(QueueState::WAKE_ALLS);
        while state.get(QueueState::STATE) == State::Woken {
            match self.compare_exchange(state, state.with_state(State::Empty)) {
                Ok(_) => return Poll::Ready(Ok(())),
                Err(actual) => state = actual,
            }
        }

        match state.get(QueueState::STATE) {
            State::Closed => crate::closed(),
            _ if state.get(QueueState::WAKE_ALLS) > initial_wake_alls => Poll::Ready(Ok(())),
            State::Empty | State::Waiting => Poll::Pending,
            State::Woken => Poll::Ready(Ok(())),
        }
    }

    /// Asynchronously poll the given function `f` until a condition occurs,
    /// using the [`WaitQueue`] to only re-poll when notified.
    ///
    /// This can be used to implement a "wait loop", turning a "try" function
    /// (e.g. "try_recv" or "try_send") into an asynchronous function (e.g.
    /// "recv" or "send").
    ///
    /// In particular, this function correctly *registers* interest in the [`WaitQueue`]
    /// prior to polling the function, ensuring that there is not a chance of a race
    /// where the condition occurs AFTER checking but BEFORE registering interest
    /// in the [`WaitQueue`], which could lead to deadlock.
    ///
    /// This is intended to have similar behavior to `Condvar` in the standard library,
    /// but asynchronous, and not requiring operating system intervention (or existence).
    ///
    /// In particular, this can be used in cases where interrupts or events are used
    /// to signify readiness or completion of some task, such as the completion of a
    /// DMA transfer, or reception of an ethernet frame. In cases like this, the interrupt
    /// can wake the queue, allowing the polling function to check status fields for
    /// partial progress or completion.
    ///
    /// Consider using [`Self::wait_for_value()`] if your function does return a value.
    ///
    /// Consider using [`WaitCell::wait_for()`](super::wait_cell::WaitCell::wait_for)
    /// if you do not need multiple waiters.
    ///
    /// # Returns
    ///
    /// * [`Ok`]`(())` if the closure returns `true`.
    /// * [`Err`]`(`[`Closed`](crate::Closed)`)` if the [`WaitQueue`] is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use std::sync::Arc;
    /// use maitake_sync::WaitQueue;
    /// use std::sync::atomic::{AtomicU8, Ordering};
    ///
    /// let queue = Arc::new(WaitQueue::new());
    /// let num = Arc::new(AtomicU8::new(0));
    ///
    /// let waiter1 = task::spawn({
    ///     // clone items to move into the spawned task
    ///     let queue = queue.clone();
    ///     let num = num.clone();
    ///     async move {
    ///         queue.wait_for(|| num.load(Ordering::Relaxed) > 5).await;
    ///         println!("received wakeup!");
    ///     }
    /// });
    ///
    /// let waiter2 = task::spawn({
    ///     // clone items to move into the spawned task
    ///     let queue = queue.clone();
    ///     let num = num.clone();
    ///     async move {
    ///         queue.wait_for(|| num.load(Ordering::Relaxed) > 10).await;
    ///         println!("received wakeup!");
    ///     }
    /// });
    ///
    /// println!("poking task...");
    ///
    /// for i in 0..20 {
    ///     num.store(i, Ordering::Relaxed);
    ///     queue.wake();
    /// }
    ///
    /// waiter1.await.unwrap();
    /// waiter2.await.unwrap();
    /// # }
    /// # test();
    /// ```
    pub async fn wait_for<F: FnMut() -> bool>(&self, mut f: F) -> WaitResult<()> {
        loop {
            let wait = self.wait();
            let mut wait = core::pin::pin!(wait);
            let _ = wait.as_mut().subscribe()?;
            if f() {
                return Ok(());
            }
            wait.await?;
        }
    }

    /// Asynchronously poll the given function `f` until a condition occurs,
    /// using the [`WaitQueue`] to only re-poll when notified.
    ///
    /// This can be used to implement a "wait loop", turning a "try" function
    /// (e.g. "try_recv" or "try_send") into an asynchronous function (e.g.
    /// "recv" or "send").
    ///
    /// In particular, this function correctly *registers* interest in the [`WaitQueue`]
    /// prior to polling the function, ensuring that there is not a chance of a race
    /// where the condition occurs AFTER checking but BEFORE registering interest
    /// in the [`WaitQueue`], which could lead to deadlock.
    ///
    /// This is intended to have similar behavior to `Condvar` in the standard library,
    /// but asynchronous, and not requiring operating system intervention (or existence).
    ///
    /// In particular, this can be used in cases where interrupts or events are used
    /// to signify readiness or completion of some task, such as the completion of a
    /// DMA transfer, or reception of an ethernet frame. In cases like this, the interrupt
    /// can wake the queue, allowing the polling function to check status fields for
    /// partial progress or completion, and also return the status flags at the same time.
    ///
    /// Consider using [`Self::wait_for()`] if your function does not return a value.
    ///
    /// Consider using [`WaitCell::wait_for_value()`](super::wait_cell::WaitCell::wait_for_value)
    /// if you do not need multiple waiters.
    ///
    /// * [`Ok`]`(T)` if the closure returns [`Some`]`(T)`.
    /// * [`Err`]`(`[`Closed`](crate::Closed)`)` if the [`WaitQueue`] is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// use std::sync::Arc;
    /// use maitake_sync::WaitQueue;
    /// use std::sync::atomic::{AtomicU8, Ordering};
    ///
    /// let queue = Arc::new(WaitQueue::new());
    /// let num = Arc::new(AtomicU8::new(0));
    ///
    /// let waiter1 = task::spawn({
    ///     // clone items to move into the spawned task
    ///     let queue = queue.clone();
    ///     let num = num.clone();
    ///     async move {
    ///         let rxd = queue.wait_for_value(|| {
    ///             let val = num.load(Ordering::Relaxed);
    ///             if val > 5 {
    ///                 return Some(val);
    ///             }
    ///             None
    ///         }).await.unwrap();
    ///         assert!(rxd > 5);
    ///         println!("received wakeup with value: {rxd}");
    ///     }
    /// });
    ///
    /// let waiter2 = task::spawn({
    ///     // clone items to move into the spawned task
    ///     let queue = queue.clone();
    ///     let num = num.clone();
    ///     async move {
    ///         let rxd = queue.wait_for_value(|| {
    ///             let val = num.load(Ordering::Relaxed);
    ///             if val > 10 {
    ///                 return Some(val);
    ///             }
    ///             None
    ///         }).await.unwrap();
    ///         assert!(rxd > 10);
    ///         println!("received wakeup with value: {rxd}");
    ///     }
    /// });
    ///
    /// println!("poking task...");
    ///
    /// for i in 0..20 {
    ///     num.store(i, Ordering::Relaxed);
    ///     queue.wake();
    /// }
    ///
    /// waiter1.await.unwrap();
    /// waiter2.await.unwrap();
    /// # }
    /// # test();
    /// ```
    pub async fn wait_for_value<T, F: FnMut() -> Option<T>>(&self, mut f: F) -> WaitResult<T> {
        loop {
            let wait = self.wait();
            let mut wait = core::pin::pin!(wait);
            match wait.as_mut().subscribe() {
                Poll::Ready(wr) => wr?,
                Poll::Pending => {}
            }
            if let Some(t) = f() {
                return Ok(t);
            }
            wait.await?;
        }
    }

    /// Returns `true` if this `WaitQueue` is [closed](Self::close).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.load().get(QueueState::STATE) == State::Closed
    }

    /// Returns a [`Waiter`] entry in this queue.
    ///
    /// This is factored out into a separate function because it's used by both
    /// [`WaitQueue::wait`] and [`WaitQueue::wait_owned`].
    fn waiter(&self) -> Waiter {
        // how many times has `wake_all` been called when this waiter is created?
        let current_wake_alls = test_dbg!(self.load().get(QueueState::WAKE_ALLS));
        let state = WaitStateBits::new()
            .with(WaitStateBits::WAKE_ALLS, current_wake_alls)
            .with(WaitStateBits::STATE, WaitState::Start);
        Waiter {
            state,
            node: UnsafeCell::new(Node {
                links: list::Links::new(),
                waker: Wakeup::Empty,
                _pin: PhantomPinned,
            }),
        }
    }

    #[cfg_attr(test, track_caller)]
    fn load(&self) -> QueueState {
        #[allow(clippy::let_and_return)]
        let state = QueueState::from_bits(self.state.load(SeqCst));
        test_debug!("state.load() = {state:?}");
        state
    }

    #[cfg_attr(test, track_caller)]
    fn store(&self, state: QueueState) {
        test_debug!("state.store({state:?}");
        self.state.store(state.0, SeqCst);
    }

    #[cfg_attr(test, track_caller)]
    fn compare_exchange(
        &self,
        current: QueueState,
        new: QueueState,
    ) -> Result<QueueState, QueueState> {
        #[allow(clippy::let_and_return)]
        let res = self
            .state
            .compare_exchange(current.0, new.0, SeqCst, SeqCst)
            .map(QueueState::from_bits)
            .map_err(QueueState::from_bits);
        test_debug!("state.compare_exchange({current:?}, {new:?}) = {res:?}");
        res
    }

    #[cold]
    #[inline(never)]
    fn wake_locked(&self, queue: &mut List<Waiter>, curr: QueueState) -> Option<Waker> {
        let state = curr.get(QueueState::STATE);

        // is the queue still in the `Waiting` state? it is possible that we
        // transitioned to a different state while locking the queue.
        if test_dbg!(state) != State::Waiting {
            // if there are no longer any queued tasks, try to store the
            // wakeup in the queue and bail.
            if let Err(actual) = self.compare_exchange(curr, curr.with_state(State::Woken)) {
                debug_assert!(actual.get(QueueState::STATE) != State::Waiting);
                self.store(actual.with_state(State::Woken));
            }

            return None;
        }

        // otherwise, we have to dequeue a task and wake it.
        let node = queue
            .pop_back()
            .expect("if we are in the Waiting state, there must be waiters in the queue");
        let waker = Waiter::wake(node, queue, Wakeup::One);

        // if we took the final waiter currently in the queue, transition to the
        // `Empty` state.
        if test_dbg!(queue.is_empty()) {
            self.store(curr.with_state(State::Empty));
        }

        waker
    }

    /// Drain waiters from `queue` and add them to `batch`. Returns `true` if
    /// the batch was filled while more waiters remain in the queue, indicating
    /// that this function must be called again to wake all waiters.
    fn drain_to_wake_batch(
        batch: &mut WakeBatch,
        queue: &mut List<Waiter>,
        wakeup: Wakeup,
    ) -> bool {
        while let Some(node) = queue.pop_back() {
            let Some(waker) = Waiter::wake(node, queue, wakeup.clone()) else {
                // this waiter was enqueued by `Wait::register` and doesn't have
                // a waker, just keep going.
                continue;
            };

            if batch.add_waker(waker) {
                // there's still room in the wake set, just keep adding to it.
                continue;
            }

            // wake set is full, drop the lock and wake everyone!
            break;
        }

        !queue.is_empty()
    }
}

// === impl Waiter ===

impl Waiter {
    /// Returns the [`Waker`] for the task that owns this `Waiter`.
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
    fn wake(this: NonNull<Self>, list: &mut List<Self>, wakeup: Wakeup) -> Option<Waker> {
        Waiter::with_node(this, list, |node| {
            let waker = test_dbg!(mem::replace(&mut node.waker, wakeup));
            match waker {
                // the node has a registered waker, so wake the task.
                Wakeup::Waiting(waker) => Some(waker),
                // do nothing: the node was registered by `Wait::register`
                // without a waker, so the future will already be woken when it is
                // actually polled.
                Wakeup::Empty => None,
                // the node was already woken? this should not happen and
                // probably indicates a race!
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
        f: impl FnOnce(&mut Node) -> T,
    ) -> T {
        unsafe {
            // safety: this is only called while holding the lock on the queue,
            // so it's safe to mutate the waiter.
            this.as_mut().node.with_mut(|node| f(&mut *node))
        }
    }

    fn poll_wait<Lock>(
        mut self: Pin<&mut Self>,
        queue: &WaitQueue<Lock>,
        waker: Option<&Waker>,
    ) -> Poll<WaitResult<()>>
    where
        Lock: ScopedRawMutex,
    {
        test_debug!(ptr = ?fmt::ptr(self.as_mut()), "Waiter::poll_wait");
        let ptr = unsafe { NonNull::from(Pin::into_inner_unchecked(self.as_mut())) };
        let mut this = self.as_mut().project();

        match test_dbg!(this.state.get(WaitStateBits::STATE)) {
            WaitState::Start => {
                let queue_state = queue.load();

                // can we consume a pending wakeup?
                if queue
                    .compare_exchange(
                        queue_state.with_state(State::Woken),
                        queue_state.with_state(State::Empty),
                    )
                    .is_ok()
                {
                    this.state.set(WaitStateBits::STATE, WaitState::Woken);
                    return Poll::Ready(Ok(()));
                }

                // okay, no pending wakeups. try to wait...

                test_debug!("poll_wait: locking...");
                queue.queue.with_lock(move |waiters| {
                    test_debug!("poll_wait: -> locked");
                    let mut queue_state = queue.load();

                    // the whole queue was woken while we were trying to acquire
                    // the lock!
                    if queue_state.get(QueueState::WAKE_ALLS)
                        != this.state.get(WaitStateBits::WAKE_ALLS)
                    {
                        this.state.set(WaitStateBits::STATE, WaitState::Woken);
                        return Poll::Ready(Ok(()));
                    }

                    // transition the queue to the waiting state
                    'to_waiting: loop {
                        match test_dbg!(queue_state.get(QueueState::STATE)) {
                            // the queue is `Empty`, transition to `Waiting`
                            State::Empty => {
                                match queue.compare_exchange(
                                    queue_state,
                                    queue_state.with_state(State::Waiting),
                                ) {
                                    Ok(_) => break 'to_waiting,
                                    Err(actual) => queue_state = actual,
                                }
                            }
                            // the queue is already `Waiting`
                            State::Waiting => break 'to_waiting,
                            // the queue was woken, consume the wakeup.
                            State::Woken => {
                                match queue.compare_exchange(
                                    queue_state,
                                    queue_state.with_state(State::Empty),
                                ) {
                                    Ok(_) => {
                                        this.state.set(WaitStateBits::STATE, WaitState::Woken);
                                        return Poll::Ready(Ok(()));
                                    }
                                    Err(actual) => queue_state = actual,
                                }
                            }
                            State::Closed => return crate::closed(),
                        }
                    }

                    // enqueue the node
                    this.state.set(WaitStateBits::STATE, WaitState::Waiting);
                    if let Some(waker) = waker {
                        this.node.as_mut().with_mut(|node| {
                            unsafe {
                                // safety: we may mutate the node because we are
                                // holding the lock.
                                debug_assert!(matches!((*node).waker, Wakeup::Empty));
                                (*node).waker = Wakeup::Waiting(waker.clone());
                            }
                        });
                    }

                    waiters.push_front(ptr);

                    Poll::Pending
                })
            }
            WaitState::Waiting => {
                queue.queue.with_lock(|_waiters| {
                    this.node.with_mut(|node| unsafe {
                        // safety: we may mutate the node because we are
                        // holding the lock.
                        let node = &mut *node;
                        match node.waker {
                            Wakeup::Waiting(ref mut curr_waker) => {
                                match waker {
                                    Some(waker) if !curr_waker.will_wake(waker) => {
                                        *curr_waker = waker.clone()
                                    }
                                    _ => {}
                                }
                                Poll::Pending
                            }
                            Wakeup::All | Wakeup::One => {
                                this.state.set(WaitStateBits::STATE, WaitState::Woken);
                                Poll::Ready(Ok(()))
                            }
                            Wakeup::Closed => {
                                this.state.set(WaitStateBits::STATE, WaitState::Woken);
                                crate::closed()
                            }
                            Wakeup::Empty => {
                                if let Some(waker) = waker {
                                    node.waker = Wakeup::Waiting(waker.clone());
                                }

                                Poll::Pending
                            }
                        }
                    })
                })
            }
            WaitState::Woken => Poll::Ready(Ok(())),
        }
    }

    /// Release this `Waiter` from the queue.
    ///
    /// This is called from the `drop` implementation for the [`Wait`] and
    /// [`WaitOwned`] futures.
    fn release<Lock>(mut self: Pin<&mut Self>, queue: &WaitQueue<Lock>)
    where
        Lock: ScopedRawMutex,
    {
        let state = *(self.as_mut().project().state);
        let ptr = NonNull::from(unsafe { Pin::into_inner_unchecked(self) });
        test_debug!(self = ?fmt::ptr(ptr), ?state, ?queue.state, "Waiter::release");

        // if we're not enqueued, we don't have to do anything else.
        if state.get(WaitStateBits::STATE) != WaitState::Waiting {
            return;
        }

        let next_waiter = queue.queue.with_lock(|waiters| {
            let state = queue.load();
            // remove the node
            unsafe {
                // safety: we have the lock on the queue, so this is safe.
                waiters.remove(ptr);
            };

            // if we removed the last waiter from the queue, transition the state to
            // `Empty`.
            if test_dbg!(waiters.is_empty()) && state.get(QueueState::STATE) == State::Waiting {
                queue.store(state.with_state(State::Empty));
            }

            // if the node has an unconsumed wakeup, it must be assigned to the next
            // node in the queue.
            if Waiter::with_node(ptr, waiters, |node| matches!(&node.waker, Wakeup::One)) {
                queue.wake_locked(waiters, state)
            } else {
                None
            }
        });

        if let Some(next) = next_waiter {
            next.wake();
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

// === impl Wait ===

impl<Lock: ScopedRawMutex> Wait<'_, Lock> {
    /// Returns `true` if this `Wait` future is waiting for a notification from
    /// the provided [`WaitQueue`].
    ///
    /// # Examples
    ///
    /// ```
    /// use maitake_sync::WaitQueue;
    ///
    /// let queue1 = WaitQueue::new();
    /// let queue2 = WaitQueue::new();
    ///
    /// let wait = queue1.wait();
    /// assert!(wait.waits_on(&queue1));
    /// assert!(!wait.waits_on(&queue2));
    /// ```
    #[inline]
    #[must_use]
    pub fn waits_on(&self, queue: &WaitQueue<Lock>) -> bool {
        ptr::eq(self.queue, queue)
    }

    /// Returns `true` if `self` and `other` are waiting on a notification from
    /// the same [`WaitQueue`].
    ///
    /// # Examples
    ///
    /// Two [`Wait`] futures waiting on the same [`WaitQueue`] return `true`:
    ///
    /// ```
    /// use maitake_sync::WaitQueue;
    ///
    /// let queue = WaitQueue::new();
    ///
    /// let wait1 = queue.wait();
    /// let wait2 = queue.wait();
    /// assert!(wait1.same_queue(&wait2));
    /// assert!(wait2.same_queue(&wait1));
    /// ```
    ///
    /// Two [`Wait`] futures waiting on different [`WaitQueue`]s return `false`:
    ///
    /// ```
    /// use maitake_sync::WaitQueue;
    ///
    /// let queue1 = WaitQueue::new();
    /// let queue2 = WaitQueue::new();
    ///
    /// let wait1 = queue1.wait();
    /// let wait2 = queue2.wait();
    /// assert!(!wait1.same_queue(&wait2));
    /// assert!(!wait2.same_queue(&wait1));
    /// ```
    #[inline]
    #[must_use]
    pub fn same_queue(&self, other: &Wait<'_, Lock>) -> bool {
        ptr::eq(self.queue, other.queue)
    }

    /// Eagerly subscribe this future to wakeups from [`WaitQueue::wake()`].
    ///
    /// Polling a `Wait` future adds that future to the list of waiters that may
    /// receive a wakeup from a `WaitQueue`. However, in some cases, it is
    /// desirable to subscribe to wakeups *prior* to actually waiting for one.
    /// This method should be used when it is necessary to ensure a `Wait`
    /// future is in the list of waiters before the future is `poll`ed for the
    /// first time.
    ///
    /// In general, this method is used in cases where a [`WaitQueue`] must
    /// synchronize with some additional state, such as an `AtomicBool` or
    /// counter. If a task first checks that state, and then chooses whether or
    /// not to wait on the `WaitQueue` based on that state, then a race
    /// condition may occur where the `WaitQueue` wakes waiters *between* when
    /// the task checked the external state and when it first polled its `Wait`
    /// future to wait on the queue. This method allows registering the `Wait`
    /// future with the queue *prior* to checking the external state, without
    /// actually sleeping, so that when the task does wait for the `Wait` future
    /// to complete, it will have received any wakeup that was sent between when
    /// the external state was checked and the `Wait` future was first polled.
    ///
    /// # Returns
    ///
    /// This method returns a [`Poll`]`<`[`WaitResult`]`>` which is `Ready` a wakeup was
    /// already received. This method returns [`Poll::Ready`] in the following
    /// cases:
    ///
    ///  1. The [`WaitQueue::wake()`] method was called between the creation of the
    ///     `Wait` and the call to this method.
    ///  2. This is the first call to `subscribe` or `poll` on this future, and the
    ///     `WaitQueue` was holding a stored wakeup from a previous call to
    ///     [`wake()`]. This method consumes the wakeup in that case.
    ///  3. The future has previously been `subscribe`d or polled, and it has since
    ///     then been marked ready by either consuming a wakeup from the
    ///     `WaitQueue`, or by a call to [`wake()`] or [`wake_all()`] that
    ///     removed it from the list of futures ready to receive wakeups.
    ///  4. The `WaitQueue` has been [`close`d](WaitQueue::close), in which case
    ///     this method returns `Poll::Ready(Err(Closed))`.
    ///
    /// If this method returns [`Poll::Ready`], any subsequent `poll`s of this
    /// `Wait` future will also immediately return [`Poll::Ready`].
    ///
    /// If the [`Wait`] future subscribed to wakeups from the queue, and
    /// has not been woken, this method returns [`Poll::Pending`].
    ///
    /// [`wake()`]: WaitQueue::wake
    /// [`wake_all()`]: WaitQueue::wake_all
    pub fn subscribe(self: Pin<&mut Self>) -> Poll<WaitResult<()>> {
        let this = self.project();
        this.waiter.poll_wait(this.queue, None)
    }
}

impl<Lock: ScopedRawMutex> Future for Wait<'_, Lock> {
    type Output = WaitResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.waiter.poll_wait(this.queue, Some(cx.waker()))
    }
}

#[pinned_drop]
impl<Lock: ScopedRawMutex> PinnedDrop for Wait<'_, Lock> {
    fn drop(mut self: Pin<&mut Self>) {
        let this = self.project();
        this.waiter.release(this.queue);
    }
}

// === impl QueueState ===

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
            _ => unsafe {
                unreachable_unchecked!("all potential 2-bit patterns should be covered!")
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

// === impl WaitOwned ===

feature! {
    #![feature = "alloc"]

    use alloc::sync::Arc;

    /// Future returned from [`WaitQueue::wait_owned()`].
    ///
    /// This is identical to the [`Wait`] future, except that it takes an
    /// [`Arc`] reference to the [`WaitQueue`], allowing the returned future to
    /// live for the `'static` lifetime.
    ///
    /// This future is fused, so once it has completed, any future calls to poll
    /// will immediately return [`Poll::Ready`].
    ///
    /// # Notes
    ///
    /// This future is `!Unpin`, as it is unsafe to [`core::mem::forget`] a
    /// `WaitOwned`  future once it has been polled. For instance, the following
    /// code must not compile:
    ///
    ///```compile_fail
    /// use maitake_sync::wait_queue::WaitOwned;
    ///
    /// // Calls to this function should only compile if `T` is `Unpin`.
    /// fn assert_unpin<T: Unpin>() {}
    ///
    /// assert_unpin::<WaitOwned<'_>>();
    /// ```
    #[derive(Debug)]
    #[pin_project(PinnedDrop)]
    pub struct WaitOwned<Lock: ScopedRawMutex = DefaultMutex> {
        /// The `WaitQueue` being waited on.
        queue: Arc<WaitQueue<Lock>>,

        /// Entry in the wait queue.
        #[pin]
        waiter: Waiter,
    }

    impl<Lock: ScopedRawMutex> WaitQueue<Lock> {
        /// Wait to be woken up by this queue, returning a future that's valid
        /// for the `'static` lifetime.
        ///
        /// This returns a [`WaitOwned`] future that will complete when the task
        /// is woken by a call to [`wake()`] or [`wake_all()`], or when the
        /// `WaitQueue` is [closed].
        ///
        /// This is identical to the [`wait()`] method, except that it takes a
        /// [`Arc`] reference to the [`WaitQueue`], allowing the returned future
        /// to live for the `'static` lifetime. See the documentation for
        /// [`wait()`] for details on how to use the future returned by this
        /// method.
        ///
        /// # Returns
        ///
        /// The [`Future`] returned by this method completes with one of the
        /// following [outputs](Future::Output):
        ///
        /// - [`Ok`]`(())` if the task was woken by a call to [`wake()`] or
        ///   [`wake_all()`].
        /// - [`Err`]`(`[`Closed`]`)` if the task was woken by the `WaitQueue`
        ///   being [closed].
        ///
        /// # Cancellation
        ///
        /// A `WaitQueue` fairly distributes wakeups to waiting tasks in the
        /// order that they started to wait. If a [`WaitOwned`] future is
        /// dropped, the task will forfeit its position in the queue.
        ///
        /// [`wake()`]: Self::wake
        /// [`wake_all()`]: Self::wake_all
        /// [`wait()`]: Self::wait
        /// [closed]: Self::close
        /// [`Closed`]: crate::Closed
        pub fn wait_owned(self: &Arc<Self>) -> WaitOwned<Lock> {
            let waiter = self.waiter();
            let queue = self.clone();
            WaitOwned { queue, waiter }
        }
    }

    // === impl WaitOwned ===

    impl<Lock: ScopedRawMutex> WaitOwned<Lock> {
        /// Returns `true` if this `WaitOwned` future is waiting for a
        /// notification from the provided [`WaitQueue`].
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake_sync::WaitQueue;
        /// use std::sync::Arc;
        ///
        /// let queue1 = Arc::new(WaitQueue::new());
        /// let queue2 = Arc::new(WaitQueue::new());
        ///
        /// let wait = queue1.clone().wait_owned();
        /// assert!(wait.waits_on(&queue1));
        /// assert!(!wait.waits_on(&queue2));
        /// ```
        #[inline]
        #[must_use]
        pub fn waits_on(&self, queue: &WaitQueue<Lock>) -> bool {
            ptr::eq(&*self.queue, queue)
        }

        /// Returns `true` if `self` and `other` are waiting on a notification
        /// from the same [`WaitQueue`].
        ///
        /// # Examples
        ///
        /// Two [`WaitOwned`] futures waiting on the same [`WaitQueue`] return
        /// `true`:
        ///
        /// ```
        /// use maitake_sync::WaitQueue;
        /// use std::sync::Arc;
        ///
        /// let queue = Arc::new(WaitQueue::new());
        ///
        /// let wait1 = queue.clone().wait_owned();
        /// let wait2 = queue.clone().wait_owned();
        /// assert!(wait1.same_queue(&wait2));
        /// assert!(wait2.same_queue(&wait1));
        /// ```
        ///
        /// Two [`WaitOwned`] futures waiting on different [`WaitQueue`]s return
        /// `false`:
        ///
        /// ```
        /// use maitake_sync::WaitQueue;
        /// use std::sync::Arc;
        ///
        /// let queue1 = Arc::new(WaitQueue::new());
        /// let queue2 = Arc::new(WaitQueue::new());
        ///
        /// let wait1 = queue1.wait_owned();
        /// let wait2 = queue2.wait_owned();
        /// assert!(!wait1.same_queue(&wait2));
        /// assert!(!wait2.same_queue(&wait1));
        /// ```
        #[inline]
        #[must_use]
        pub fn same_queue(&self, other: &WaitOwned<Lock>) -> bool {
            Arc::ptr_eq(&self.queue, &other.queue)
        }

        /// Eagerly subscribe this future to wakeups from [`WaitQueue::wake()`].
        ///
        /// Polling a `WaitOwned` future adds that future to the list of waiters
        /// that may receive a wakeup from a `WaitQueue`. However, in some
        /// cases, it is desirable to subscribe to wakeups *prior* to actually
        /// waiting for one. This method should be used when it is necessary to
        /// ensure a `WaitOwned` future is in the list of waiters before the
        /// future is `poll`ed for the rst time.
        ///
        /// In general, this method is used in cases where a [`WaitQueue`] must
        /// synchronize with some additional state, such as an `AtomicBool` or
        /// counter. If a task first checks that state, and then chooses whether or
        /// not to wait on the `WaitQueue` based on that state, then a race
        /// condition may occur where the `WaitQueue` wakes waiters *between* when
        /// the task checked the external state and when it first polled its
        /// `WaitOwned` future to wait on the queue. This method allows
        /// registering the `WaitOwned`  future with the queue *prior* to
        /// checking the external state, without actually sleeping, so that when
        /// the task does wait for the `WaitOwned` future to complete, it will
        /// have received any wakeup that was sent between when the external
        /// state was checked and the `WaitOwned` future was first polled.
        ///
        /// # Returns
        ///
        /// This method returns a [`Poll`]`<`[`WaitResult`]`>` which is `Ready`
        /// a wakeup was already received. This method returns [`Poll::Ready`]
        /// in the following cases:
        ///
        ///  1. The [`WaitQueue::wake()`] method was called between the creation
        ///     of the `WaitOwned` future and the call to this method.
        ///  2. This is the first call to `subscribe` or `poll` on this future,
        ///     and the `WaitQueue` was holding a stored wakeup from a previous
        ///     call to [`wake()`]. This method consumes the wakeup in that case.
        ///  3. The future has previously been `subscribe`d or polled, and it
        ///     has since then been marked ready by either consuming a wakeup
        ///     from the `WaitQueue`, or by a call to [`wake()`] or
        ///     [`wake_all()`] that removed it from the list of futures ready to
        ///     receive wakeups.
        ///  4. The `WaitQueue` has been [`close`d](WaitQueue::close), in which
        ///     case this method returns `Poll::Ready(Err(Closed))`.
        ///
        /// If this method returns [`Poll::Ready`], any subsequent `poll`s of
        /// this `Wait` future will also immediately return [`Poll::Ready`].
        ///
        /// If the [`WaitOwned`] future subscribed to wakeups from the queue,
        /// and has not been woken, this method returns [`Poll::Pending`].
        ///
        /// [`wake()`]: WaitQueue::wake
        /// [`wake_all()`]: WaitQueue::wake_all
        pub fn subscribe(self: Pin<&mut Self>) -> Poll<WaitResult<()>> {
            let this = self.project();
            this.waiter.poll_wait(this.queue, None)
        }
    }

    impl<Lock: ScopedRawMutex> Future for WaitOwned<Lock> {
        type Output = WaitResult<()>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.project();
            this.waiter.poll_wait(&*this.queue, Some(cx.waker()))
        }
    }

    #[pinned_drop]
    impl<Lock: ScopedRawMutex> PinnedDrop for WaitOwned<Lock> {
        fn drop(mut self: Pin<&mut Self>) {
            let this = self.project();
            this.waiter.release(&*this.queue);
        }
    }
}
