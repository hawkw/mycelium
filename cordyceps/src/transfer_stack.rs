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
    pub fn push(&self, element: T::Handle) {
        let ptr = T::into_ptr(element);
        let links = unsafe { T::links(ptr).as_mut() };
        debug_assert!(links.next.with(|next| unsafe { (*next).is_none() }));

        let mut head = self.head.load(Relaxed);
        loop {
            links.next.with_mut(|next| unsafe {
                *next = NonNull::new(head);
            });

            match self
                .head
                .compare_exchange_weak(head, ptr.as_ptr(), AcqRel, Acquire)
            {
                Ok(_) => return,
                Err(actual) => head = actual,
            }
        }
    }

    pub fn pop_all(&self) -> Drain<T> {
        let head = self.head.swap(ptr::null_mut(), AcqRel);
        let next = NonNull::new(head);
        Drain { next }
    }
}

// === impl Drain ===

impl<T> Iterator for Drain<T>
where
    T: Linked<Links<T>>,
{
    type Item = T::Handle;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.next.take()?;
        unsafe {
            // advance the iterator to the next node after the current one (if
            // there is one).
            self.next = T::links(curr).as_mut().next.with_mut(|next| (*next).take());

            // return the current node
            Some(T::from_ptr(curr))
        }
    }
}
