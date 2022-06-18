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
        (self.list.len(), Some(self.list.len()))
    }
}

impl<'a, T: Linked<Links<T>> + ?Sized> Cursor<'a, T> {
    fn next_ptr(&mut self) -> Link<T> {
        let curr = self.curr.take()?;
        self.curr = unsafe { T::links(curr).as_ref().next() };
        Some(curr)
    }

    /// Moves the cursor position to the next element in the [`List`].
    ///
    /// If the cursor is pointing at the null element, this moves it to the first
    /// element in the [`List`]. If it is pointing to the last element in the
    /// list, then this will move it to the null element.
    pub fn move_next(&mut self) {
        match self.curr.take() {
            // Advance the cursor to the current node's next element.
            Some(curr) => unsafe {
                self.curr = T::links(curr).as_ref().next();
            },
            // We have no current element --- move to the start of the list.
            None => {
                self.curr = self.list.head;
            }
        }
    }

    /// Moves the cursor to the previous element in the [`List`].
    ///
    /// If the cursor is pointing at the null element, this moves it to the last
    /// element in the [`List`]. If it is pointing to the first element in the
    /// list, then this will move it to the null element.
    // XXX(eliza): i would have named this "move_back", personally, but
    // `std::collections::LinkedList`'s cursor interface calls this
    // "move_prev"...
    pub fn move_prev(&mut self) {
        match self.curr.take() {
            // Advance the cursor to the current node's prev element.
            Some(curr) => unsafe {
                self.curr = T::links(curr).as_ref().prev();
            },
            // We have no current element --- move to the end of the list.
            None => {
                self.curr = self.list.tail;
            }
        }
    }

    /// Removes the current element from the [`List`] and returns the [`Handle`]
    /// owning that element.
    ///
    /// If the cursor is currently pointing to an element, that element is
    /// removed and returned, and the cursor is moved to point to the next
    /// element in the [`List`].
    ///
    /// If the cursor is currently pointing to the null element, then no element
    /// is removed and `None` is returned.
    ///
    /// [`Handle`]: crate::Linked::Handle
    pub fn remove_current(&mut self) -> Option<T::Handle> {
        let node = self.curr?;
        unsafe {
            // before modifying `node`'s links, set the current element to the
            // one after `node`.
            self.curr = T::links(node).as_ref().next();
            // safety: `List::remove` is unsafe to call, because the caller must
            // guarantee that the removed node is part of *that* list. in this
            // case, because the cursor can only access nodes from the list it
            // points to, we know this is safe.
            self.list.remove(node)
        }
    }

    /// Find and remove the first element matching the provided `predicate`.
    ///
    /// This traverses the list from the cursor's current position and calls
    /// `predicate` with each element in the list. If `predicate` returns
    /// `true` for a given element, that element is removed from the list and
    /// returned, and the traversal ends. If the traversal reaches the end of
    /// the list without finding a match, then no element is returned.
    ///
    /// Note that if the cursor is not at the beginning of the list, then any
    /// matching elements *before* the cursor's position will not be removed.
    ///
    /// This method may be called multiple times to remove more than one
    /// matching element.
    pub fn remove_first(&mut self, mut predicate: impl FnMut(&T) -> bool) -> Option<T::Handle> {
        while !predicate(unsafe { self.curr?.as_ref() }) {
            // if the current element does not match, advance to the next node
            // in the list.
            self.move_next();
        }

        // if we have broken out of the loop without returning a `None`, remove
        // the current element.
        self.remove_current()
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
    /// position in the list.
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

    /// Returns the length of the [`List`] this cursor points to.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Returns `true` if the [`List`] this cursor points to is empty
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
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
    unsafe fn pin_node_mut(&mut self, mut node: NonNull<T>) -> Pin<&mut T> {
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
            .finish()
    }
}
