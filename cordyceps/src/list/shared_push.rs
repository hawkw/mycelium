use super::*;
use crate::loom::{
    cell::UnsafeCell,
    sync::atomic::{AtomicPtr, Ordering::*},
};

pub struct SharedPushList<T: Linked<Links<T>>> {
    head: AtomicPtr<T>,
    tail: UnsafeCell<Link<T>>,
}

impl<T: Linked<Links<T>>> SharedPushList<T> {
    pub fn push_front(&self, item: T::Handle) {
        let node = T::into_ptr(item);
        unsafe {
            // Safety: a `Handle` has
            T::links(node).as_mut().set_prev(None);
        }

        let mut head = self.head.load(Relaxed);
        loop {
            unsafe {
                T::links(node).as_mut().set_next(NonNull::new(head));
            }

            match self
                .head
                .compare_exchange_weak(head, node.as_ptr(), AcqRel, Acquire)
            {
                // Pushed the new head successfully!
                Ok(_) => break,
                // Keep trying...
                Err(actual) => head = actual,
            }
        }

        // Update the previous head's prev pointer, if there was a previous
        // head. Otherwise, set the new node as the tail, because it's the only
        // node in the list.
        match NonNull::new(head) {
            Some(head) => unsafe {
                // Safety:
                T::links(head).as_mut().set_prev(Some(node));
            },
            None => unsafe {
                // Safety: only the thread that successfully pushed the first
                // element to an empty list will write to the tail.
                self.tail.with_mut(|tail| {
                    *tail = Some(node);
                });
            },
        }
    }
}
