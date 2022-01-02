//! A lock-free, intrusive singly-linked MPSC queue.
//!
//! Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
//! [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue

use super::Linked;
use crate::sync::{
    self,
    atomic::{AtomicPtr, Ordering::*},
};
use core::{
    marker::PhantomPinned,
    mem::ManuallyDrop,
    ptr::{self, NonNull},
};

#[derive(Debug)]
pub struct Queue<T: Linked<Links = Links<T>>> {
    /// The head of the queue. This is accessed in both `enqueue` and `dequeue`.
    head: AtomicPtr<T>,
    tail: AtomicPtr<T>,
    stub: T::Handle,
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
}

impl<T: Linked<Links = Links<T>>> Queue<T> {
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
            head: AtomicPtr::new(ptr),
            tail: AtomicPtr::new(ptr),
            stub,
        }
    }

    pub fn enqueue(&self, node: T::Handle) {
        let node = ManuallyDrop::new(node);
        let ptr = T::as_ptr(&node);

        debug_assert!(!unsafe { T::links(ptr).as_ref() }.is_stub);

        self.enqueue_inner(ptr)
    }

    fn enqueue_inner(&self, ptr: NonNull<T>) {
        let links = unsafe { T::links(ptr).as_ref() };
        links.next.store(ptr::null_mut(), Relaxed);

        let ptr = ptr.as_ptr();
        if let Some(prev) = ptr::NonNull::new(self.head.swap(ptr, AcqRel)) {
            unsafe { T::links(prev).as_ref().next.store(ptr, Release) }
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
    /// The [`Queue::dequeue`] method will instead wait (by spinning with an
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
    /// may call `try_dequeue` at a time!
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    pub unsafe fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
        let mut tail = NonNull::new(self.tail.load(Relaxed)).ok_or(TryDequeueError::Empty)?;
        let mut next = T::links(tail).as_ref().next.load(Acquire);

        if tail == T::as_ptr(&self.stub) {
            debug_assert!(T::links(tail).as_ref().is_stub);

            let next_node = NonNull::new(next).ok_or(TryDequeueError::Empty)?;
            self.tail.store(next, Relaxed);
            tail = next_node;
            next = T::links(next_node).as_ref().next.load(Acquire);
        }

        if !next.is_null() {
            self.tail.store(next, Relaxed);
            return Ok(T::from_ptr(tail));
        }

        let head = self.head.load(Acquire);

        if tail.as_ptr() != head {
            return Err(TryDequeueError::Inconsistent);
        }

        self.enqueue_inner(T::as_ptr(&self.stub));

        next = T::links(tail).as_ref().next.load(Acquire);
        if next.is_null() {
            return Err(TryDequeueError::Empty);
        }

        self.tail.store(next, Relaxed);

        debug_assert!(!T::links(tail).as_ref().is_stub);
        Ok(T::from_ptr(tail))
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
    pub unsafe fn dequeue(&self) -> Option<T::Handle> {
        let mut boff = sync::Backoff::new();
        loop {
            match self.try_dequeue() {
                Ok(val) => return Some(val),
                Err(TryDequeueError::Empty) => return None,
                Err(TryDequeueError::Inconsistent) => boff.spin(),
            }
        }
    }
}

impl<T: Linked<Links = Links<T>>> Drop for Queue<T> {
    fn drop(&mut self) {
        // All atomic operations in the `Drop` impl can be `Relaxed`, because we
        // have exclusive ownership of the queue --- if we are dropping it, no
        // one else is enqueueing new nodes.
        let mut current = self.tail.load(Relaxed);
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
    T: Linked<Links = Links<T>>,
    T::Handle: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T> Send for Queue<T>
where
    T: Send + Linked<Links = Links<T>>,
    T::Handle: Send,
{
}
unsafe impl<T: Send + Linked<Links = Links<T>>> Sync for Queue<T> {}

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
            let q = Arc::new(q);

            let threads: Vec<_> = (0..threads)
                .map(|thread| {
                    let q = q.clone();
                    thread::spawn(move || {
                        for i in 0..msgs {
                            q.enqueue(entry(i + (thread * 10)));
                            info!(thread, "enqueue msg {}/{}", i, msgs);
                        }
                    })
                })
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

    unsafe impl Linked for Entry {
        type Handle = Pin<Box<Entry>>;
        type Links = Links<Self>;

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
