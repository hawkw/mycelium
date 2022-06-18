use super::{Link, Links, List};
use crate::{util::FmtOption, Linked};
use core::{fmt, pin::Pin, ptr::NonNull};

/// A cursor over a [`List`].
///
/// This is similar to a mutable iterator (and implements the [`Iterator`]
/// trait), but it also permits modification to the list itself.
pub struct Cursor<'list, T: Linked<Links<T>> + ?Sized> {
    pub(super) list: &'list mut List<T>,
    pub(super) curr: Link<T>,
    pub(super) len: usize,
}

// === impl Cursor ====

impl<'a, T: Linked<Links<T>> + ?Sized> Iterator for Cursor<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_ptr().map(|mut ptr| unsafe {
            // safety: it is safe for us to mutate `curr`, because the cursor
            // mutably borrows the `List`, ensuring that the list will not be dropped
            // while the iterator exists. the returned item will not outlive the
            // cursor, and no one else can mutate it, as we have exclusive
            // access to the list..
            ptr.as_mut()
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T: Linked<Links<T>> + ?Sized> Cursor<'a, T> {
    fn next_ptr(&mut self) -> Link<T> {
        let curr = self.curr.take()?;
        self.curr = unsafe { T::links(curr).as_ref().next() };
        Some(curr)
    }

    /// Find and remove the first element matching the provided `predicate`.
    ///
    /// This traverses the list from the cursor's current position and calls
    /// `predicate` with each element in the list. If `predicate` returns
    /// `true` for a given element, that element is removed from the list and
    /// returned, and the traversal ends. If the entire list is traversed
    /// without finding a matching element, this returns `None`.
    ///
    /// This method may be called multiple times to remove more than one
    /// matching element.
    pub fn remove_first(&mut self, mut predicate: impl FnMut(&T) -> bool) -> Option<T::Handle> {
        let mut item = None;
        while let Some(node) = self.next_ptr() {
            if predicate(unsafe { node.as_ref() }) {
                item = Some(node);
                self.len -= 1;
                break;
            }
        }
        unsafe { self.list.remove(item?) }
    }

    /// Borrows the element that the cursor is currently pointing at.
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn current(&self) -> Option<Pin<&T>> {
        // NOTE(eliza): in this case, we don't *need* to pin the reference,
        // because it's immutable and you can't move out of a shared
        // reference in safe code. but...it makes the API more consistent
        // with `front_mut` etc.
        self.curr.map(|node| unsafe { self.pin_node(node) })
    }

    /// Mutably borrows the element that the cursor is currently pointing at.
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn current_mut(&mut self) -> Option<Pin<&mut T>> {
        self.curr.map(|node| unsafe { self.pin_node_mut(node) })
    }

    /// Borrows the next element after the cursor's current position in the
    /// list.
    ///
    /// If the cursor is pointing to the null element, this returns the first
    /// element in the [`List`]. If the cursor is pointing to the last element
    /// in the [`List`], this returns `None`.
    pub fn peek_next(&self) -> Option<Pin<&T>> {
        unsafe {
            let next = match self.curr {
                Some(curr) => T::links(curr).as_ref().next(),
                None => self.list.head,
            };
            next.map(|next| self.pin_node(next))
        }
    }

    /// Borrows the previous element before the cursor's current position in the
    /// list.
    ///
    /// If the cursor is pointing to the null element, this returns the last
    /// element in the [`List`]. If the cursor is pointing to the first element
    /// in the [`List`], this returns `None`.
    // XXX(eliza): i would have named this "move_back", personally, but
    // `std::collections::LinkedList`'s cursor interface calls this
    // "move_prev"...
    pub fn peek_prev(&self) -> Option<Pin<&T>> {
        unsafe {
            let prev = match self.curr {
                Some(curr) => T::links(curr).as_ref().prev(),
                None => self.list.tail,
            };
            prev.map(|prev| self.pin_node(prev))
        }
    }

    /// Mutably borrows the next element after the cursor's current position in
    /// the list.
    ///
    /// If the cursor is pointing to the null element, this returns the first
    /// element in the [`List`]. If the cursor is pointing to the last element
    /// in the [`List`], this returns `None`.
    pub fn peek_next_mut(&mut self) -> Option<Pin<&mut T>> {
        unsafe {
            let next = match self.curr {
                Some(curr) => T::links(curr).as_mut().next(),
                None => self.list.head,
            };
            next.map(|next| self.pin_node_mut(next))
        }
    }

    /// Mutably borrows the previous element before the cursor's current
    /// position in the  list.
    ///
    /// If the cursor is pointing to the null element, this returns the last
    /// element in the [`List`]. If the cursor is pointing to the first element
    /// in the [`List`], this returns `None`.
    // XXX(eliza): i would have named this "move_back", personally, but
    // `std::collections::LinkedList`'s cursor interface calls this
    // "move_prev"...
    pub fn peek_prev_mut(&mut self) -> Option<Pin<&mut T>> {
        unsafe {
            let prev = match self.curr {
                Some(curr) => T::links(curr).as_mut().prev(),
                None => self.list.tail,
            };
            prev.map(|prev| self.pin_node_mut(prev))
        }
    }

    /// # Safety
    ///
    /// - `node` must point to an element currently in this list.
    unsafe fn pin_node(&self, node: NonNull<T>) -> Pin<&T> {
        // safety: elements in the list must be pinned while they are in the
        // list, so it is safe to construct a `pin` here provided that the
        // `Linked` trait's invariants are upheld.
        Pin::new_unchecked(node.as_ref())
    }

    /// # Safety
    ///
    /// - `node` must point to an element currently in this list.
    unsafe fn pin_node_mut(&mut self, node: NonNull<T>) -> Pin<&mut T> {
        // safety: elements in the list must be pinned while they are in the
        // list, so it is safe to construct a `pin` here provided that the
        // `Linked` trait's invariants are upheld.
        Pin::new_unchecked(node.as_mut())
    }
}

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for Cursor<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cursor")
            .field("curr", &FmtOption::new(&self.curr))
            .field("list", &self.list)
            .field("len", &self.len)
            .finish()
    }
}
