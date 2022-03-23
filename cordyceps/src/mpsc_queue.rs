//! An intrusive singly-linked lock-free MPSC queue.
//!
//! Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
//!
//! [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue

use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, AtomicPtr, Ordering::*},
    },
    util::{Backoff, CachePadded},
    Linked,
};
use core::{
    fmt,
    marker::PhantomPinned,
    ptr::{self, NonNull},
};

/// An intrusive singly-linked lock-free MPSC queue.
///
/// Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
///
/// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
pub struct MpscQueue<T: Linked<Links<T>>> {
    /// The head of the queue. This is accessed in both `enqueue` and `dequeue`.
    head: CachePadded<AtomicPtr<T>>,

    /// The tail of the queue. This is accessed only when dequeueing.
    tail: CachePadded<UnsafeCell<*mut T>>,

    /// Does a consumer handle to the queue exist? If not, it is safe to create a
    /// new consumer.
    has_consumer: CachePadded<AtomicBool>,

    stub: NonNull<T>,
}

/// A handle that holds the right to dequeue elements from a [`Queue`].
///
/// This can be used when one thread wishes to dequeue many elements at a time,
/// to avoid the overhead of ensuring mutual exclusion on every [`dequeue`] or
/// [`try_dequeue`] call.
///
/// This type is returned by the [`Queue::consume`] and [`Queue::try_consume`]
/// methods.
///
/// If the right to dequeue elements needs to be reserved for longer than a
/// single scope, an owned variant ([`OwnedConsumer`]) is also available, when
/// the [`Queue`] is stored in an [`Arc`]. Since the [`Queue`] must be stored
/// in an [`Arc`], the [`OwnedConsumer`] type requires the "alloc" feature flag.
///
/// [`dequeue`]: Consumer::dequeue
/// [`try_dequeue`]: Consumer::try_dequeue
/// [`Arc`]: alloc::sync::Arc
pub struct Consumer<'q, T: Linked<Links<T>>> {
    q: &'q MpscQueue<T>,
}

#[derive(Debug)]
pub struct Links<T> {
    /// The next node in the queue.
    next: AtomicPtr<T>,

    /// Used for debug mode consistency checking only.
    #[cfg(debug_assertions)]
    is_stub: AtomicBool,
    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// https://github.com/rust-lang/rust/issues/63818.
    _unpin: PhantomPinned,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TryDequeueError {
    Empty,
    Inconsistent,
    Busy,
}

// === impl Queue ===

impl<T: Linked<Links<T>>> MpscQueue<T> {
    pub fn new() -> Self
    where
        T::Handle: Default,
    {
        Self::new_with_stub(Default::default())
    }

    pub fn new_with_stub(stub: T::Handle) -> Self {
        let stub = T::into_ptr(stub);

        // // In debug mode, set the stub flag for consistency checking.
        #[cfg(debug_assertions)]
        unsafe {
            (*T::links(stub).as_ptr()).is_stub.store(true, Release);
        }
        let ptr = stub.as_ptr();

        Self {
            head: CachePadded(AtomicPtr::new(ptr)),
            tail: CachePadded(UnsafeCell::new(ptr)),
            has_consumer: CachePadded(AtomicBool::new(false)),
            stub,
        }
    }

    pub fn enqueue(&self, node: T::Handle) {
        let ptr = T::into_ptr(node);

        debug_assert!(!unsafe { T::links(ptr).as_ref() }.is_stub());

        self.enqueue_inner(ptr)
    }

    #[inline]
    fn enqueue_inner(&self, ptr: NonNull<T>) {
        unsafe {
            (*T::links(ptr).as_ptr())
                .next
                .store(ptr::null_mut(), Relaxed)
        };

        let ptr = ptr.as_ptr();
        let prev = self.head.swap(ptr, AcqRel);
        unsafe {
            // Safety: in release mode, we don't null check `prev`. This is
            // because no pointer in the list should ever be a null pointer, due
            // to the presence of the stub node.
            (*T::links(non_null(prev)).as_ptr())
                .next
                .store(ptr, Release);
        }
    }

    /// Try to dequeue an element from the queue, without waiting if the queue
    /// is in an inconsistent state, or until there is no other consumer trying
    /// to read from the queue.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an incosistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method returns
    /// [`TryDequeueError::Inconsistent`] when the queue is in an inconsistent
    /// state.
    ///
    /// Additionally, because this is a multi-producer, _single-consumer_ queue,
    /// only one thread may be dequeueing at a time. If another thread is
    /// dequeueing, this method returns [`TryDequeueError::Busy`].
    ///
    /// The [`Queue::dequeue`] method will instead wait (by spinning with an
    /// exponential backoff) when the queue is in an inconsistent state or busy.
    ///
    /// The unsafe [`Queue::try_dequeue_unchecked`] method will not check if the
    /// queue is busy before dequeueing an element. This can be used when the
    /// user code guarantees that no other threads will dequeue from the queue
    /// concurrently, but this cannot be enforced by the compiler.
    ///
    /// # Returns
    ///
    /// - `T::Handle` if an element was successfully dequeued
    /// - [`TryDequeueError::Empty`] if there are no elements in the queue
    /// - [`TryDequeueError::Inconsistent`] if the queue is currently in an
    ///   inconsistent state
    /// - [`TryDequeueError::Busy`] if another thread is currently trying to
    ///   dequeue a message.
    pub fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
        if self
            .has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            return Err(TryDequeueError::Busy);
        }

        let res = unsafe {
            // Safety: the `has_consumer` flag ensures mutual exclusion of
            // consumers.
            self.try_dequeue_unchecked()
        };

        self.has_consumer.store(false, Release);
        res
    }

    /// Dequeue an element from the queue.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an incosistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method will wait by spinning
    /// with an exponential backoff if the queue is in an inconsistent state.
    ///
    /// Additionally, because this is a multi-producer, _single-consumer_ queue,
    /// only one thread may be dequeueing at a time. If another thread is
    /// dequeueing, this method will spin until the queue is no longer busy.
    ///
    /// The [`Queue::try_dequeue`] will return an error rather than waiting when
    /// the queue is in an inconsistent state or busy.
    ///
    /// The unsafe [`Queue::dequeue_unchecked`] method will not check if the
    /// queue is busy before dequeueing an element. This can be used when the
    /// user code guarantees that no other threads will dequeue from the queue
    /// concurrently, but this cannot be enforced by the compiler.
    ///
    /// # Returns
    ///
    /// - `Some(T::Handle)` if an element was successfully dequeued
    /// - `None` if the queue is empty or another thread is dequeueing
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    pub fn dequeue(&self) -> Option<T::Handle> {
        let mut boff = Backoff::new();
        loop {
            match self.try_dequeue() {
                Ok(val) => return Some(val),
                Err(TryDequeueError::Empty) => return None,
                Err(_) => boff.spin(),
            }
        }
    }

    /// Returns a [`Consumer`] handle that reserves the exclusive right to dequeue
    /// elements from the queue until it is dropped.
    ///
    /// If another thread is dequeueing, this method spins until there is no
    /// other thread dequeueing.
    pub fn consume(&self) -> Consumer<'_, T> {
        self.lock_consumer();
        Consumer { q: self }
    }

    /// Attempts to reserve a [`Consumer`] handle that holds the exclusive right
    /// to dequeue  elements from the queue until it is dropped.
    ///
    /// If another thread is dequeueing, this returns `None` instead.
    pub fn try_consume(&self) -> Option<Consumer<'_, T>> {
        // lock the consumer-side of the queue.
        self.try_lock_consumer().map(|_| Consumer { q: self })
    }

    /// Try to dequeue an element from the queue, without waiting if the queue
    /// is in an inconsistent state.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an incosistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method returns
    /// [`TryDequeueError::Inconsistent`] when the queue is in an inconsistent
    /// state.
    ///
    /// The [`Queue::dequeue_unchecked`] method will instead wait (by spinning with an
    /// exponential backoff) when the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `T::Handle` if an element was successfully dequeued
    /// - [`TryDequeueError::Empty`] if there are no elements in the queue
    /// - [`TryDequeueError::Inconsistent`] if the queue is currently in an
    ///   inconsistent state
    ///
    /// # Safety
    ///
    /// This is a multi-producer, *single-consumer* queue. Only one thread/core
    /// may call `try_dequeue_unchecked` at a time!
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    pub unsafe fn try_dequeue_unchecked(&self) -> Result<T::Handle, TryDequeueError> {
        self.tail.with_mut(|tail| {
            let mut tail_node = NonNull::new(*tail).ok_or(TryDequeueError::Empty)?;
            let mut next = (*T::links(tail_node).as_ptr()).next.load(Acquire);

            if ptr::eq(
                tail_node.as_ptr() as *const _,
                self.stub.as_ptr() as *const _,
            ) {
                debug_assert!(T::links(tail_node).as_ref().is_stub());
                let next_node = NonNull::new(next).ok_or(TryDequeueError::Empty)?;

                *tail = next;
                tail_node = next_node;
                next = (*T::links(next_node).as_ptr()).next.load(Acquire);
            }

            if !next.is_null() {
                *tail = next;
                return Ok(T::from_ptr(tail_node));
            }

            let head = self.head.load(Acquire);

            if tail_node.as_ptr() != head {
                return Err(TryDequeueError::Inconsistent);
            }

            self.enqueue_inner(self.stub);

            next = T::links(tail_node).as_ref().next.load(Acquire);
            if next.is_null() {
                return Err(TryDequeueError::Empty);
            }

            *tail = next;

            debug_assert!(!T::links(tail_node).as_ref().is_stub());
            Ok(T::from_ptr(tail_node))
        })
    }

    /// Dequeue an element from the queue.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an incosistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method will wait by spinning
    /// with an exponential backoff if the queue is in an inconsistent state.
    ///
    /// The [`Queue::try_dequeue`] will return an error rather than waiting when
    /// the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `Some(T::Handle)` if an element was successfully dequeued
    /// - `None` if the queue is empty
    ///
    /// # Safety
    ///
    /// This is a multi-producer, *single-consumer* queue. Only one thread/core
    /// may call `dequeue` at a time!
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    pub unsafe fn dequeue_unchecked(&self) -> Option<T::Handle> {
        let mut boff = Backoff::new();
        loop {
            match self.try_dequeue_unchecked() {
                Ok(val) => return Some(val),
                Err(TryDequeueError::Empty) => return None,
                Err(TryDequeueError::Inconsistent) => boff.spin(),
                Err(TryDequeueError::Busy) => {
                    unreachable!("try_dequeue_unchecked never returns `Busy`!")
                }
            }
        }
    }

    #[inline]
    fn lock_consumer(&self) {
        let mut boff = Backoff::new();
        while self
            .has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            while self.has_consumer.load(Relaxed) {
                boff.spin();
            }
        }
    }

    #[inline]
    fn try_lock_consumer(&self) -> Option<()> {
        self.has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .map(|_| ())
            .ok()
    }
}

impl<T: Linked<Links<T>>> Drop for MpscQueue<T> {
    fn drop(&mut self) {
        let mut current = self.tail.with_mut(|tail| unsafe {
            // Safety: because `Drop` is called with `&mut self`, we have
            // exclusive ownership over the queue, so it's always okay to touch
            // the tail cell.
            *tail
        });
        while let Some(node) = NonNull::new(current) {
            unsafe {
                let links = T::links(node).as_ref();
                let next = links.next.load(Relaxed);

                // Skip dropping the stub node; it is owned by the queue and
                // will be dropped when the queue is dropped. If we dropped it
                // here, that would cause a double free!
                if !ptr::eq(node.as_ptr() as *const _, self.stub.as_ptr() as *const _) {
                    // Convert the pointer to the owning handle and drop it.
                    debug_assert!(!links.is_stub());
                    drop(T::from_ptr(node));
                } else {
                    debug_assert!(links.is_stub());
                }

                current = next;
            }
        }

        unsafe {
            drop(T::from_ptr(self.stub));
        }
    }
}

impl<T> fmt::Debug for MpscQueue<T>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Queue")
            .field("head", &format_args!("{:p}", self.head.load(Acquire)))
            // only the consumer can load the tail; trying to print it here
            // could be racy.
            // XXX(eliza): we could replace the `UnsafeCell` with an atomic,
            // and then it would be okay to print the tail...but then we would
            // lose loom checking for tail accesses...
            .field("tail", &format_args!("..."))
            .field("has_consumer", &self.has_consumer.load(Acquire))
            .finish()
    }
}

impl<T> Default for MpscQueue<T>
where
    T: Linked<Links<T>>,
    T::Handle: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T> Send for MpscQueue<T>
where
    T: Send + Linked<Links<T>>,
    T::Handle: Send,
{
}
unsafe impl<T: Send + Linked<Links<T>>> Sync for MpscQueue<T> {}

// === impl Consumer ===

impl<'q, T: Send + Linked<Links<T>>> Consumer<'q, T> {
    /// Dequeue an element from the queue.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an incosistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method will wait by spinning
    /// with an exponential backoff if the queue is in an inconsistent state.
    ///
    /// The [`Consumer::try_dequeue`] will return an error rather than waiting when
    /// the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `Some(T::Handle)` if an element was successfully dequeued
    /// - `None` if the queue is empty
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    #[inline]
    pub fn dequeue(&self) -> Option<T::Handle> {
        debug_assert!(self.q.has_consumer.load(Acquire));
        unsafe {
            // Safety: we have reserved exclusive access to the queue.
            self.q.dequeue_unchecked()
        }
    }

    /// Try to dequeue an element from the queue, without waiting if the queue
    /// is in an inconsistent state.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an incosistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method returns
    /// [`TryDequeueError::Inconsistent`] when the queue is in an inconsistent
    /// state.
    ///
    /// The [`Consumer::dequeue`] method will instead wait (by spinning with an
    /// exponential backoff) when the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `T::Handle` if an element was successfully dequeued
    /// - [`TryDequeueError::Empty`] if there are no elements in the queue
    /// - [`TryDequeueError::Inconsistent`] if the queue is currently in an
    ///   inconsistent state
    ///
    ///
    /// # Returns
    ///
    /// - `Some(T::Handle)` if an element was successfully dequeued
    /// - `None` if the queue is empty
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    #[inline]
    pub fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
        debug_assert!(self.q.has_consumer.load(Acquire));
        unsafe {
            // Safety: we have reserved exclusive access to the queue.
            self.q.try_dequeue_unchecked()
        }
    }
}

impl<T: Linked<Links<T>>> Drop for Consumer<'_, T> {
    fn drop(&mut self) {
        self.q.has_consumer.store(false, Release);
    }
}

impl<T> fmt::Debug for Consumer<'_, T>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tail = self.q.tail.with(|tail| unsafe {
            // Safety: it's okay for the consumer to access the tail cell, since
            // we have exclusive access to it.
            *tail
        });
        f.debug_struct("Consumer")
            .field("head", &format_args!("{:p}", tail))
            // only the consumer can load the tail; trying to print it here
            // could be racy.
            // XXX(eliza): we could replace the `UnsafeCell` with an atomic,
            // and then it would be okay to print the tail...but then we would
            // lose loom checking for tail accesses...
            .field("tail", &tail)
            .field("has_consumer", &self.q.has_consumer.load(Acquire))
            .finish()
    }
}

// === impl Links ===

impl<T> Links<T> {
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: AtomicBool::new(false),
        }
    }

    #[cfg(loom)]
    pub fn new() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: AtomicBool::new(false),
        }
    }

    #[cfg(debug_assertions)]
    fn is_stub(&self) -> bool {
        self.is_stub.load(Acquire)
    }
}

impl<T> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

crate::feature! {
    #![feature = "alloc"]

    use alloc::sync::Arc;

    /// An owned handle that holds the right to dequeue elements from the queue.
    ///
    /// This can be used when one thread wishes to dequeue many elements at a time,
    /// to avoid the overhead of ensuring mutual exclusion on every [`dequeue`] or
    /// [`try_dequeue`] call.
    ///
    /// This type is returned by the [`Queue::consume_owned`] and
    /// [`Queue::try_consume_owned`] methods.
    ///
    /// This is similar to the [`Consumer`] type, but the queue is stored in an
    /// [`Arc`] rather than borrowed. This allows a single `OwnedConsumer`
    /// instance to be stored in a struct and used indefinitely.
    ///
    /// Since the queue is stored in an [`Arc`], this requires the `alloc`
    /// feature flag to be enabled.
    ///
    /// [`dequeue`]: OwnedConsumer::dequeue
    /// [`try_dequeue`]: OwnedConsumer::try_dequeue
    /// [`Arc`]: alloc::sync::Arc
    pub struct OwnedConsumer<T: Linked<Links<T>>> {
        q: Arc<MpscQueue<T>>
    }

    // === impl Consumer ===

    impl<T: Linked<Links<T>>> OwnedConsumer<T> {
        /// Dequeue an element from the queue.
        ///
        /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
        /// is possible for this queue design to enter an incosistent state if the
        /// consumer tries to dequeue an element while a producer is in the middle
        /// of enqueueing a new element. If this occurs, the consumer must briefly
        /// wait before dequeueing an element. This method will wait by spinning
        /// with an exponential backoff if the queue is in an inconsistent state.
        ///
        /// The [`Consumer::try_dequeue`] will return an error rather than waiting when
        /// the queue is in an inconsistent state.
        ///
        /// # Returns
        ///
        /// - `Some(T::Handle)` if an element was successfully dequeued
        /// - `None` if the queue is empty
        ///
        /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
        #[inline]
        pub fn dequeue(&self) -> Option<T::Handle> {
            debug_assert!(self.q.has_consumer.load(Acquire));
            unsafe {
                // Safety: we have reserved exclusive access to the queue.
                self.q.dequeue_unchecked()
            }
        }

        /// Try to dequeue an element from the queue, without waiting if the queue
        /// is in an inconsistent state.
        ///
        /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
        /// is possible for this queue design to enter an incosistent state if the
        /// consumer tries to dequeue an element while a producer is in the middle
        /// of enqueueing a new element. If this occurs, the consumer must briefly
        /// wait before dequeueing an element. This method returns
        /// [`TryDequeueError::Inconsistent`] when the queue is in an inconsistent
        /// state.
        ///
        /// The [`Consumer::dequeue`] method will instead wait (by spinning with an
        /// exponential backoff) when the queue is in an inconsistent state.
        ///
        /// # Returns
        ///
        /// - `T::Handle` if an element was successfully dequeued
        /// - [`TryDequeueError::Empty`] if there are no elements in the queue
        /// - [`TryDequeueError::Inconsistent`] if the queue is currently in an
        ///   inconsistent state
        ///
        ///
        /// # Returns
        ///
        /// - `Some(T::Handle)` if an element was successfully dequeued
        /// - `None` if the queue is empty
        ///
        /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
        #[inline]
        pub fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
            debug_assert!(self.q.has_consumer.load(Acquire));
            unsafe {
                // Safety: we have reserved exclusive access to the queue.
                self.q.try_dequeue_unchecked()
            }
        }
    }

    impl<T: Linked<Links<T>>> Drop for OwnedConsumer<T> {
        fn drop(&mut self) {
            self.q.has_consumer.store(false, Release);
        }
    }

    impl<T: Linked<Links<T>>> fmt::Debug for OwnedConsumer<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let tail = self.q.tail.with(|tail| unsafe {
                // Safety: it's okay for the consumer to access the tail cell, since
                // we have exclusive access to it.
               *tail
            });
            f.debug_struct("OwnedConsumer")
                .field("head", &self.q.head.load(Acquire))
                // only the consumer can load the tail; trying to print it here
                // could be racy.
                // XXX(eliza): we could replace the `UnsafeCell` with an atomic,
                // and then it would be okay to print the tail...but then we would
                // lose loom checking for tail accesses...
                .field("tail", &tail)
                .field("has_consumer", &self.q.has_consumer.load(Acquire))
                .finish()
        }
    }

    // === impl Queue ===

    impl<T: Linked<Links<T>>> MpscQueue<T> {
        /// Returns a [`OwnedConsumer`] handle that reserves the exclusive right to dequeue
        /// elements from the queue until it is dropped.
        ///
        /// If another thread is dequeueing, this method spins until there is no
        /// other thread dequeueing.
        pub fn consume_owned(self: Arc<Self>) -> OwnedConsumer<T> {
            self.lock_consumer();
            OwnedConsumer { q: self }
        }

        /// Attempts to reserve an [`OwnedConsumer`] handle that holds the exclusive right
        /// to dequeue  elements from the queue until it is dropped.
        ///
        /// If another thread is dequeueing, this returns `None` instead.
        pub fn try_consume_owned(self: Arc<Self>) -> Option<OwnedConsumer<T>> {
            self.try_lock_consumer().map(|_| OwnedConsumer { q: self })
        }
    }
}

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use crate::loom::{self, sync::Arc, thread};
    use test_util::*;

    #[test]
    fn basically_works_loom() {
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS * 2;
        const TOTAL_MSGS: i32 = THREADS * MSGS;
        basically_works_test(THREADS, MSGS, TOTAL_MSGS);
    }

    #[test]
    fn doesnt_leak() {
        // Test that dropping the queue drops any messages that haven't been
        // consumed by the consumer.
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS * 2;
        // Only consume half as many messages as are sent, to ensure dropping
        // the queue does not leak.
        const TOTAL_MSGS: i32 = (THREADS * MSGS) / 2;
        basically_works_test(THREADS, MSGS, TOTAL_MSGS);
    }

    fn basically_works_test(threads: i32, msgs: i32, total_msgs: i32) {
        loom::model(move || {
            let stub = entry(666);
            let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));

            let threads: Vec<_> = (0..threads)
                .map(|thread| thread::spawn(do_tx(thread, msgs, &q)))
                .collect();

            let mut i = 0;
            while i < total_msgs {
                match q.try_dequeue() {
                    Ok(val) => {
                        i += 1;
                        tracing::info!(?val, "dequeue {}/{}", i, total_msgs);
                    }
                    Err(TryDequeueError::Busy) => panic!(
                        "the queue should never be busy, as there is only a single consumer!"
                    ),
                    Err(err) => {
                        tracing::info!(?err, "dequeue error");
                        thread::yield_now();
                    }
                }
            }

            for thread in threads {
                thread.join().unwrap();
            }
        })
    }

    fn do_tx(thread: i32, msgs: i32, q: &Arc<MpscQueue<Entry>>) -> impl FnOnce() + Send + Sync {
        let q = q.clone();
        move || {
            for i in 0..msgs {
                q.enqueue(entry(i + (thread * 10)));
                tracing::info!(thread, "enqueue msg {}/{}", i, msgs);
            }
        }
    }

    #[test]
    fn mpmc() {
        // Tests multiple consumers competing for access to the consume side of
        // the queue.
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS;

        fn do_rx(thread: i32, q: Arc<MpscQueue<Entry>>) {
            let mut i = 0;
            while let Some(val) = q.dequeue() {
                tracing::info!(?val, ?thread, "dequeue {}/{}", i, THREADS * MSGS);
                i += 1;
            }
        }

        loom::model(|| {
            let stub = entry(666);
            let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));

            let mut threads: Vec<_> = (0..THREADS)
                .map(|thread| thread::spawn(do_tx(thread, MSGS, &q)))
                .collect();

            threads.push(thread::spawn({
                let q = q.clone();
                move || do_rx(THREADS + 1, q)
            }));
            do_rx(THREADS + 2, q);

            for thread in threads {
                thread.join().unwrap();
            }
        })
    }
}

#[cfg(debug_assertions)]
#[track_caller]
#[inline(always)]
unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new(ptr).expect(
        "/!\\ constructed a `NonNull` from a null pointer! /!\\ \n\
        in release mode, this would have called `NonNull::new_unchecked`, \
        violating the `NonNull` invariant! this is a bug in `cordyceps!`.",
    )
}

#[cfg(not(debug_assertions))]
#[inline(always)]
unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new_unchecked(ptr)
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use test_util::*;

    #[test]
    fn dequeue_empty() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);
        assert_eq!(q.dequeue(), None)
    }

    #[test]
    fn try_dequeue_empty() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);
        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Empty))
    }

    #[test]
    fn try_dequeue_busy() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);

        let consumer = q.try_consume().expect("must acquire consumer");
        assert_eq!(consumer.try_dequeue(), Err(TryDequeueError::Empty));

        q.enqueue(entry(1));

        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Busy));

        assert_eq!(consumer.try_dequeue(), Ok(entry(1)),);

        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Busy));

        assert_eq!(consumer.try_dequeue(), Err(TryDequeueError::Empty));

        drop(consumer);
        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Empty));
    }

    #[test]
    fn enqueue_dequeue() {
        let stub = entry(666);
        let e = entry(1);
        let q = MpscQueue::<Entry>::new_with_stub(stub);
        q.enqueue(e);
        assert_eq!(q.dequeue(), Some(entry(1)));
        assert_eq!(q.dequeue(), None)
    }

    #[test]
    fn basically_works() {
        use std::{sync::Arc, thread};

        const THREADS: i32 = if_miri(3, 8);
        const MSGS: i32 = if_miri(10, 1000);

        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);

        assert_eq!(q.dequeue(), None);

        let q = Arc::new(q);

        let threads: Vec<_> = (0..THREADS)
            .map(|thread| {
                let q = q.clone();
                thread::spawn(move || {
                    for i in 0..MSGS {
                        q.enqueue(entry(i));
                        println!("thread {}; msg {}/{}", thread, i, MSGS);
                    }
                })
            })
            .collect();

        let mut i = 0;
        while i < THREADS * MSGS {
            match q.try_dequeue() {
                Ok(msg) => {
                    i += 1;
                    println!("recv {:?} ({}/{})", msg, i, THREADS * MSGS);
                }
                Err(TryDequeueError::Busy) => {
                    panic!("the queue should never be busy, as there is only one consumer")
                }
                Err(e) => {
                    println!("recv error {:?}", e);
                    thread::yield_now();
                }
            }
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }

    const fn if_miri(miri: i32, not_miri: i32) -> i32 {
        if cfg!(miri) {
            miri
        } else {
            not_miri
        }
    }
}

#[cfg(test)]
mod test_util {
    use super::*;
    use crate::loom::alloc;
    use std::pin::Pin;

    #[derive(Debug)]
    #[repr(C)]
    pub(super) struct Entry {
        links: Links<Entry>,
        pub(super) val: i32,
        // participate in loom leak checking
        _track: alloc::Track<()>,
    }

    impl std::cmp::PartialEq for Entry {
        fn eq(&self, other: &Self) -> bool {
            self.val == other.val
        }
    }

    unsafe impl<'a> Linked<Links<Self>> for Entry {
        type Handle = Pin<Box<Entry>>;

        fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
            unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
        }

        unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
            // Safety: if this function is only called by the linked list
            // implementation (and it is not intended for external use), we can
            // expect that the `NonNull` was constructed from a reference which
            // was pinned.
            //
            // If other callers besides `List`'s internals were to call this on
            // some random `NonNull<Entry>`, this would not be the case, and
            // this could be constructing an erroneous `Pin` from a referent
            // that may not be pinned!
            Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
        }

        unsafe fn links(target: NonNull<Entry>) -> NonNull<Links<Entry>> {
            // Safety: this cast is safe only because `Entry` `is repr(C)` and
            // is the first field.
            target.cast()
        }
    }

    pub(super) fn entry(val: i32) -> Pin<Box<Entry>> {
        Box::pin(Entry {
            links: Links::new(),
            val,
            _track: alloc::Track::new(()),
        })
    }
}
