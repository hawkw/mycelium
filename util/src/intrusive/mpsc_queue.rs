//! A lock-free, intrusive singly-linked MPSC queue.
//!
//! Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
//! [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue

use super::Linked;
use crate::{
    fmt,
    sync::{
        self,
        atomic::{AtomicPtr, Ordering::*},
    },
};
use core::{
    marker::PhantomPinned,
    mem::ManuallyDrop,
    ptr::{self, NonNull},
};

#[derive(Debug)]
pub struct Queue<T: Linked<Links = Links<T>>> {
    head: AtomicPtr<T>,
    tail: AtomicPtr<T>,
    stub: T::Handle,
}

#[derive(Debug, Default)]
pub struct Links<T> {
    next: AtomicPtr<T>,
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
        let ptr = T::as_ptr(&stub).as_ptr();
        Self {
            head: AtomicPtr::new(ptr),
            tail: AtomicPtr::new(ptr),
            stub,
        }
    }

    pub fn enqueue(&self, node: T::Handle) {
        let node = ManuallyDrop::new(node);
        self.enqueue_inner(T::as_ptr(&node))
    }

    fn enqueue_inner(&self, ptr: NonNull<T>) {
        let links = unsafe { T::links(ptr).as_ref() };
        links.next.store(ptr::null_mut(), Relaxed);

        let ptr = ptr.as_ptr();
        if let Some(prev) = ptr::NonNull::new(self.head.swap(ptr, AcqRel)) {
            unsafe { T::links(prev).as_ref().next.store(ptr, Release) }
        }
    }

    pub unsafe fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
        let mut tail = NonNull::new(self.tail.load(Relaxed)).ok_or(TryDequeueError::Empty)?;
        let mut next = T::links(tail).as_ref().next.load(Acquire);

        if tail == T::as_ptr(&self.stub) {
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
        Ok(T::from_ptr(tail))
    }

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
        while !current.is_null() {
            unsafe {
                let next = (*current).next.load(Relaxed);
                // Convert the pointer to the owning handle and drop it.
                drop(T::from_ptr(current));
                current = next;
            }
        }
    }
}

unsafe impl<T: Send + Linked<Links = Links<T>>> Send for Queue<T> {}
unsafe impl<T: Send + Linked<Links = Links<T>>> Sync for Queue<T> {}

// === impl Links ===

impl<T> Links<T> {
    pub fn new() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
        }
    }
}

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use test_util::*;
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
            if let Ok(_) = unsafe { q.try_dequeue() } {
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
    use std::pin::Pin;

    #[derive(Debug, Default)]
    pub(super) struct Entry {
        links: Links<Entry>,
        val: i32,
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
        })
    }

    pub(super) fn ptr(r: &Pin<Box<Entry>>) -> NonNull<Entry> {
        r.as_ref().get_ref().into()
    }
}
