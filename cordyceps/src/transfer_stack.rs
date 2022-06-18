use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{AtomicPtr, Ordering::*},
    },
    Linked,
};
use core::{
    fmt,
    marker::PhantomPinned,
    ptr::{self, NonNull},
};

pub struct TransferStack<T> {
    head: AtomicPtr<T>,
}
pub struct Drain<T> {
    next: Option<NonNull<T>>,
}

/// Links to other nodes in a [`TransferStack`].
///
/// In order to be part of a [`TransferStack`], a type must contain an instance of this
/// type, and must implement the [`Linked`] trait for `Links<Self>`.
pub struct Links<T> {
    /// The next node in the queue.
    next: UnsafeCell<Option<NonNull<T>>>,

    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// <https://github.com/rust-lang/rust/issues/63818>.
    _unpin: PhantomPinned,
}

impl<T> TransferStack<T>
where
    T: Linked<Links<T>>,
{
    /// Returns a new `TransferStack`.
    #[cfg(not(loom))]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Returns a new `TransferStack`.
    #[cfg(loom)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub fn push(&self, element: T::Handle) {
        let ptr = T::into_ptr(element);
        test_trace!(?ptr, "TransferStack::push");

        let links = unsafe { T::links(ptr).as_mut() };
        debug_assert!(links.next.with(|next| unsafe { (*next).is_none() }));

        let mut head = self.head.load(Relaxed);
        loop {
            test_trace!(?ptr, ?head, "TransferStack::push");
            links.next.with_mut(|next| unsafe {
                *next = NonNull::new(head);
            });

            match self
                .head
                .compare_exchange_weak(head, ptr.as_ptr(), AcqRel, Acquire)
            {
                Ok(_) => {
                    test_trace!(?ptr, ?head, "TransferStack::push -> pushed");
                    return;
                }
                Err(actual) => head = actual,
            }
        }
    }

    pub fn drain(&self) -> Drain<T> {
        let head = self.head.swap(ptr::null_mut(), AcqRel);
        let next = NonNull::new(head);
        Drain { next }
    }
}

impl<T> Links<T> {
    /// Returns new [`TransferStack`] links.
    #[cfg(not(loom))]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: UnsafeCell::new(None),
            _unpin: PhantomPinned,
        }
    }

    /// Returns new [`TransferStack`] links.
    #[cfg(loom)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            next: UnsafeCell::new(None),
            _unpin: PhantomPinned,
        }
    }
}

// === impl Drain ===

impl<T> Iterator for Drain<T>
where
    T: Linked<Links<T>>,
{
    type Item = T::Handle;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.next.take();
        test_trace!(?curr, "Drain::next");
        let curr = curr?;
        unsafe {
            // advance the iterator to the next node after the current one (if
            // there is one).
            self.next = T::links(curr).as_mut().next.with_mut(|next| (*next).take());

            test_trace!(?self.next, "Drain::next");

            // return the current node
            Some(T::from_ptr(curr))
        }
    }
}

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use crate::loom::{
        self,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread,
    };
    use test_util::Entry;

    #[test]
    fn multithreaded_push() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let threads = Arc::new(AtomicUsize::new(2));
            let thread1 = thread::spawn({
                let stack = stack.clone();
                let threads = threads.clone();
                move || {
                    Entry::push_all(&stack, 1, PUSHES);
                    threads.fetch_sub(1, Ordering::Relaxed);
                }
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                let threads = threads.clone();
                move || {
                    Entry::push_all(&stack, 2, PUSHES);
                    threads.fetch_sub(1, Ordering::Relaxed);
                }
            });

            let mut seen = Vec::new();

            loop {
                seen.extend(stack.drain().map(|entry| entry.val));

                if threads.load(Ordering::Relaxed) == 0 {
                    break;
                }

                thread::yield_now();
            }

            seen.extend(stack.drain().map(|entry| entry.val));

            seen.sort();
            assert_eq!(seen, vec![10, 11, 20, 21]);

            thread1.join().unwrap();
            thread2.join().unwrap();
        })
    }

    #[test]
    fn multithreaded_pop() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let thread1 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 1, PUSHES)
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 2, PUSHES)
            });

            let thread3 = thread::spawn({
                let stack = stack.clone();
                move || stack.drain().map(|entry| entry.val).collect::<Vec<_>>()
            });

            let seen_thread0 = stack.drain().map(|entry| entry.val).collect::<Vec<_>>();
            let seen_thread3 = thread3.join().unwrap();

            thread1.join().unwrap();
            thread2.join().unwrap();

            let seen_thread0_final = stack.drain().map(|entry| entry.val).collect::<Vec<_>>();

            let mut all = dbg!(seen_thread0);
            all.extend(dbg!(seen_thread3));
            all.extend(dbg!(seen_thread0_final));

            all.sort();
            assert_eq!(all, vec![10, 11, 20, 21]);
        })
    }
}

#[cfg(test)]
mod test_util {
    use super::*;
    use core::pin::Pin;

    #[pin_project::pin_project]
    #[repr(C)]
    pub(super) struct Entry {
        #[pin]
        links: Links<Entry>,
        pub(super) val: i32,
    }

    unsafe impl Linked<Links<Self>> for Entry {
        type Handle = Pin<Box<Entry>>;

        fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Self> {
            unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
        }

        unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
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

        unsafe fn links(target: NonNull<Self>) -> NonNull<Links<Self>> {
            // Safety: this is safe because the `links` are the first field of
            // `Entry`, and `Entry` is `repr(C)`.
            target.cast()
        }
    }

    impl Entry {
        pub(super) fn new(val: i32) -> Pin<Box<Entry>> {
            Box::pin(Entry {
                links: Links::new(),
                val,
            })
        }

        pub(super) fn push_all(stack: &TransferStack<Self>, thread: i32, n: i32) {
            for i in 0..n {
                let entry = Self::new((thread * 10) + i);
                stack.push(entry);
            }
        }
    }
}
