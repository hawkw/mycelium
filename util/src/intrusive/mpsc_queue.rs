//! A lock-free, intrusive singly-linked MPSC queue.
//!
//! Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
//! [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue

use super::Linked;
use crate::{
    cell::UnsafeCell,
    sync::{
        self,
        atomic::{AtomicBool, AtomicPtr, Ordering::*},
        CachePadded,
    },
};
use core::{
    marker::PhantomPinned,
    mem::ManuallyDrop,
    ptr::{self, NonNull},
};

#[derive(Debug)]
pub struct Queue<T: Linked<Links<T>>> {
    /// The head of the queue. This is accessed in both `enqueue` and `dequeue`.
    head: CachePadded<AtomicPtr<T>>,

    /// The tail of the queue. This is accessed only when dequeueing.
    tail: CachePadded<UnsafeCell<*mut T>>,

    /// Does a consumer handle to the queue exist? If not, it is safe to create a
    /// new consumer.
    has_consumer: CachePadded<AtomicBool>,

    stub: T::Handle,
}

/// A handle that holds the right to dequeue elements from the queue.
///
/// This can be used when one thread wishes to dequeue many elements at a time,
/// to avoid the overhead of ensuring mutual exclusion on every `dequeue` or
/// `try_dequeue` call.
pub struct Consumer<'q, T: Linked<Links<T>>> {
    q: &'q Queue<T>,
}

#[derive(Debug, Default)]
pub struct Links<T> {
    next: AtomicPtr<T>,

    /// Used for debug mode consistency checking only.
    #[cfg(debug_assertions)]
    is_stub: bool,

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

impl<T: Linked<Links<T>>> Queue<T> {
    pub fn new() -> Self
    where
        T::Handle: Default,
    {
        Self::new_with_stub(Default::default())
    }

    pub fn new_with_stub(stub: T::Handle) -> Self {
        let ptr = T::as_ptr(&stub);

        // In debug mode, set the stub flag for consistency checking.
        #[cfg(debug_assertions)]
        unsafe {
            T::links(ptr).as_mut().is_stub = true;
        }

        let ptr = ptr.as_ptr();
        Self {
            head: CachePadded::new(AtomicPtr::new(ptr)),
            tail: CachePadded::new(UnsafeCell::new(ptr)),
            has_consumer: CachePadded::new(AtomicBool::new(false)),
            stub,
        }
    }

    pub fn enqueue(&self, node: T::Handle) {
        let node = ManuallyDrop::new(node);
        let ptr = T::as_ptr(&node);

        debug_assert!(!unsafe { T::links(ptr).as_ref() }.is_stub);

        self.enqueue_inner(ptr)
    }

    #[inline]
    fn enqueue_inner(&self, ptr: NonNull<T>) {
        let links = unsafe { T::links(ptr).as_ref() };
        links.next.store(ptr::null_mut(), Relaxed);

        let ptr = ptr.as_ptr();
        let prev = self.head.swap(ptr, AcqRel);
        unsafe {
            // Safety: in release mode, we don't null check `prev`. This is
            // because no pointer in the list should ever be a null pointer, due
            // to the presence of the stub node.
            T::links(non_null(prev)).as_ref().next.store(ptr, Release);
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
        let mut boff = sync::Backoff::new();
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
        let mut boff = sync::Backoff::new();
        loop {
            if let Some(consumer) = self.try_consume() {
                return consumer;
            }
            while self.has_consumer.load(Relaxed) {
                boff.spin();
            }
        }
    }

    /// Attempts to reserve a [`Consumer`] handle that holds the exclusive right
    /// to dequeue  elements from the queue until it is dropped.
    ///
    /// If another thread is dequeueing, this returns `None` instead.
    pub fn try_consume(&self) -> Option<Consumer<'_, T>> {
        // lock the consumer-side of the queue.
        self.has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .ok()?;

        Some(Consumer { q: self })
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
            let mut next = T::links(tail_node).as_ref().next.load(Acquire);

            if tail_node == T::as_ptr(&self.stub) {
                debug_assert!(T::links(tail_node).as_ref().is_stub);
                let next_node = NonNull::new(next).ok_or(TryDequeueError::Empty)?;

                *tail = next;
                tail_node = next_node;
                next = T::links(next_node).as_ref().next.load(Acquire);
            }

            if !next.is_null() {
                *tail = next;
                return Ok(T::from_ptr(tail_node));
            }

            let head = self.head.load(Acquire);

            if tail_node.as_ptr() != head {
                return Err(TryDequeueError::Inconsistent);
            }

            self.enqueue_inner(T::as_ptr(&self.stub));

            next = T::links(tail_node).as_ref().next.load(Acquire);
            if next.is_null() {
                return Err(TryDequeueError::Empty);
            }

            *tail = next;

            debug_assert!(!T::links(tail_node).as_ref().is_stub);
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
        let mut boff = sync::Backoff::new();
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
}

impl<T: Linked<Links<T>>> Drop for Queue<T> {
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
                if node != T::as_ptr(&self.stub) {
                    // Convert the pointer to the owning handle and drop it.
                    debug_assert!(!links.is_stub);
                    drop(T::from_ptr(node));
                } else {
                    debug_assert!(links.is_stub);
                }

                current = next;
            }
        }
    }
}

impl<T> Default for Queue<T>
where
    T: Linked<Links<T>>,
    T::Handle: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T> Send for Queue<T>
where
    T: Send + Linked<Links<T>>,
    T::Handle: Send,
{
}
unsafe impl<T: Send + Linked<Links<T>>> Sync for Queue<T> {}

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
    pub fn try_dequeue_unchecked(&self) -> Result<T::Handle, TryDequeueError> {
        debug_assert!(self.q.has_consumer.load(Acquire));
        unsafe {
            // Safety: we have reserved exclusive access to the queue.
            self.q.try_dequeue_unchecked()
        }
    }
}

impl<'q, T: Linked<Links<T>>> Drop for Consumer<'q, T> {
    fn drop(&mut self) {
        self.q.has_consumer.store(false, Release);
    }
}

// === impl Links ===

impl<T> Links<T> {
    pub fn new() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: false,
        }
    }
}

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use crate::loom::{self, sync::Arc, thread};
    use test_util::*;
    use tracing_01::info;

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
            let q = Arc::new(Queue::<Entry>::new_with_stub(stub));

            let threads: Vec<_> = (0..threads)
                .map(|thread| thread::spawn(do_tx(thread, msgs, &q)))
                .collect();

            let mut i = 0;
            while i < total_msgs {
                match unsafe { q.try_dequeue() } {
                    Ok(val) => {
                        i += 1;
                        info!(?val, "dequeue {}/{}", i, total_msgs);
                    }
                    Err(err) => {
                        info!(?err, "dequeue error");
                        thread::yield_now();
                    }
                }
            }

            for thread in threads {
                thread.join().unwrap();
            }
        })
    }

    fn do_tx(thread: i32, msgs: i32, q: &Arc<Queue<Entry>>) -> impl FnOnce() + Send + Sync {
        let q = q.clone();
        move || {
            for i in 0..msgs {
                q.enqueue(entry(i + (thread * 10)));
                info!(thread, "enqueue msg {}/{}", i, msgs);
            }
        }
    }

    #[test]
    fn mpmc() {
        // Tests multiple consumers competing for access to the consume side of
        // the queue.
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS * 2;

        fn do_rx(thread: i32, q: Arc<Queue<Entry>>) {
            let mut i = 0;
            while let Some(val) = q.dequeue() {
                info!(?val, ?thread, "dequeue {}/{}", i, THREADS * MSGS);
                i += 1;
            }
        }

        loom::model(|| {
            let stub = entry(666);
            let q = Arc::new(Queue::<Entry>::new_with_stub(stub));

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
        violating the `NonNull` invariant! this is a bug in `mycelium-util`.",
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
        let q = Queue::<Entry>::new_with_stub(stub);
        assert_eq!(unsafe { q.dequeue() }, None)
    }

    #[test]
    fn basically_works() {
        use std::{sync::Arc, thread};

        const THREADS: i32 = 8;
        const MSGS: i32 = 1000;

        let stub = entry(666);
        let q = Queue::<Entry>::new_with_stub(stub);

        assert_eq!(unsafe { q.dequeue() }, None);

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
            if unsafe { q.try_dequeue() }.is_ok() {
                i += 1;
                println!("recv {}/{}", i, THREADS * MSGS);
            }
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }
}

#[cfg(test)]
mod test_util {
    use super::*;
    use crate::loom::alloc;
    use std::pin::Pin;

    #[derive(Debug)]
    pub(super) struct Entry {
        links: Links<Entry>,
        val: i32,
        // participate in loom leak checking
        _track: alloc::Track<()>,
    }

    impl std::cmp::PartialEq for Entry {
        fn eq(&self, other: &Self) -> bool {
            self.val == other.val
        }
    }

    unsafe impl Linked<Links<Self>> for Entry {
        type Handle = Pin<Box<Entry>>;

        fn as_ptr(handle: &Pin<Box<Entry>>) -> NonNull<Entry> {
            NonNull::from(handle.as_ref().get_ref())
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

        unsafe fn links(mut target: NonNull<Entry>) -> NonNull<Links<Entry>> {
            NonNull::from(&mut target.as_mut().links)
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
