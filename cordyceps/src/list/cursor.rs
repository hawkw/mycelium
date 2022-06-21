use super::{Link, Links, List};
use crate::{util::FmtOption, Linked};
use core::{fmt, mem, pin::Pin, ptr::NonNull};

/// A cursor over a [`List`].
///
/// This is similar to a mutable iterator (and implements the [`Iterator`]
/// trait), but it also permits modification to the list itself.
pub struct CursorMut<'list, T: Linked<Links<T>> + ?Sized> {
    pub(super) list: &'list mut List<T>,
    pub(super) curr: Link<T>,
    pub(super) index: usize,
}

/// A cursor over a [`List`].
///
/// This is similar to a mutable iterator (and implements the [`Iterator`]
/// trait), but it also permits modification to the list itself.
///
/// # Deprecated
///
/// This is a deprecated alias for [`CursorMut`].
#[deprecated(since = "0.2.2", note = "renamed to `CursorMut`")]
pub type Cursor<'list, T> = CursorMut<'list, T>;

// === impl CursorMut ====

impl<'a, T: Linked<Links<T>> + ?Sized> Iterator for CursorMut<'a, T> {
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

    /// A [`CursorMut`] can never return an accurate `size_hint` --- its lower
    /// bound is always 0 and its upper bound is always `None`.
    ///
    /// This is because the cursor may be moved around within the list through
    /// methods outside of its `Iterator` implementation, and elements may be
    /// added or removed using the cursor. This would make any `size_hint`s a
    /// [`CursorMut`] returns inaccurate.
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<'a, T: Linked<Links<T>> + ?Sized> CursorMut<'a, T> {
    fn next_ptr(&mut self) -> Link<T> {
        let curr = self.curr.take()?;
        self.curr = unsafe { T::links(curr).as_ref().next() };
        self.index += 1;
        Some(curr)
    }

    /// Returns the index of this cursor's position in the [`List`].
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn index(&self) -> Option<usize> {
        self.curr?;
        Some(self.index)
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
                self.index += 1;
            },
            // We have no current element --- move to the start of the list.
            None => {
                self.curr = self.list.head;
                self.index = 0;
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
                // this is saturating because the current node might be the 0th
                // and we might have set `self.curr` to `None`.
                self.index = self.index.saturating_sub(1);
            },
            // We have no current element --- move to the end of the list.
            None => {
                self.curr = self.list.tail;
                self.index = self.index.checked_sub(1).unwrap_or(self.list.len());
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
        self.next_link().map(|next| unsafe { self.pin_node(next) })
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
        self.prev_link().map(|prev| unsafe { self.pin_node(prev) })
    }

    /// Mutably borrows the next element after the cursor's current position in
    /// the list.
    ///
    /// If the cursor is pointing to the null element, this returns the first
    /// element in the [`List`]. If the cursor is pointing to the last element
    /// in the [`List`], this returns `None`.
    pub fn peek_next_mut(&mut self) -> Option<Pin<&mut T>> {
        self.next_link()
            .map(|next| unsafe { self.pin_node_mut(next) })
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
        self.prev_link()
            .map(|prev| unsafe { self.pin_node_mut(prev) })
    }

    /// Inserts a new element into the [`List`] after the current one.
    ///
    /// If the cursor is pointing at the null element then the new element is
    /// inserted at the front of the [`List`].
    pub fn insert_after(&mut self, element: T::Handle) {
        let node = T::into_ptr(element);
        assert_ne!(self.curr, Some(node), "cannot insert a node after itself");
        let next = self.next_link();

        unsafe {
            self.list
                .insert_nodes_between(self.curr, next, node, node, 1);
        }

        if self.curr.is_none() {
            // The null index has shifted.
            self.index = self.list.len;
        }
    }

    /// Inserts a new element into the [`List`] before the current one.
    ///
    /// If the cursor is pointing at the null element then the new element is
    /// inserted at the front of the [`List`].
    pub fn insert_before(&mut self, element: T::Handle) {
        let node = T::into_ptr(element);
        assert_ne!(self.curr, Some(node), "cannot insert a node before itself");
        let prev = self.prev_link();

        unsafe {
            self.list
                .insert_nodes_between(prev, self.curr, node, node, 1);
        }

        self.index += 1;
    }

    /// Returns the length of the [`List`] this cursor points to.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Returns `true` if the [`List`] this cursor points to is empty
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Splits the list into two after the current element. This will return a
    /// new list consisting of everything after the cursor, with the original
    /// list retaining everything before.
    ///
    /// If the cursor is pointing at the null element, then the entire contents
    /// of the `List` are moved.
    pub fn split_after(&mut self) -> List<T> {
        let split_at = if self.index == self.list.len {
            self.index = 0;
            0
        } else {
            self.index + 1
        };
        unsafe {
            // safety: we know we are splitting at a node that belongs to our list.
            self.list.split_after_node(self.curr, split_at)
        }
    }

    /// Splits the list into two before the current element. This will return a
    /// new list consisting of everything before the cursor, with the original
    /// list retaining everything after the cursor.
    ///
    /// If the cursor is pointing at the null element, then the entire contents
    /// of the `List` are moved.
    pub fn split_before(&mut self) -> List<T> {
        let split_at = self.index;
        self.index = 0;

        let split_node = match self.curr {
            Some(node) => node,
            // the split portion is the entire list. just return it.
            None => return mem::replace(self.list, List::new()),
        };

        // the tail of the new list is the split node's `prev` node (which is
        // replaced with `None`), as the split node is the new head of this list.
        let tail = unsafe { T::links(split_node).as_mut().set_prev(None) };
        let head = if let Some(tail) = tail {
            // since `tail` is now the head of its own list, it has no `next`
            // link any more.
            let _next = unsafe { T::links(tail).as_mut().set_next(None) };
            debug_assert_eq!(_next, Some(split_node));

            // this list's head is now the split node.
            self.list.head.replace(split_node)
        } else {
            None
        };

        let split = List {
            head,
            tail,
            len: split_at,
        };

        // update this list's length (note that this occurs after constructing
        // the new list, because we use this list's length to determine the new
        // list's length).
        self.list.len -= split_at;

        split
    }

    /// Inserts all elements from `spliced` after the cursor's current position.
    ///
    /// If the cursor is pointing at the null element, then the contents of
    /// `spliced` are inserted at the beginning of the `List` the cursor points to.
    pub fn splice_after(&mut self, mut spliced: List<T>) {
        let (splice_head, splice_tail, splice_len) = match spliced.take_all() {
            Some(spliced) => spliced,
            // the spliced list is empty, do nothing.
            None => return,
        };

        let next = self.next_link();
        unsafe {
            // safety: we know `curr` and `next` came from the same list that we
            // are calling `insert_nodes_between` from, because they came from
            // this cursor, which points at `self.list`.
            self.list
                .insert_nodes_between(self.curr, next, splice_head, splice_tail, splice_len);
        }

        if self.curr.is_none() {
            self.index = self.list.len();
        }
    }

    /// Inserts all elements from `spliced` before the cursor's current position.
    ///
    /// If the cursor is pointing at the null element, then the contents of
    /// `spliced` are inserted at the end of the `List` the cursor points to.
    pub fn splice_before(&mut self, mut spliced: List<T>) {
        let (splice_head, splice_tail, splice_len) = match spliced.take_all() {
            Some(spliced) => spliced,
            // the spliced list is empty, do nothing.
            None => return,
        };

        let prev = self.prev_link();
        unsafe {
            // safety: we know `curr` and `prev` came from the same list that we
            // are calling `insert_nodes_between` from, because they came from
            // this cursor, which points at `self.list`.
            self.list
                .insert_nodes_between(prev, self.curr, splice_head, splice_tail, splice_len);
        }

        self.index += splice_len;
    }

    #[inline(always)]
    fn next_link(&self) -> Link<T> {
        match self.curr {
            Some(curr) => unsafe { T::links(curr).as_ref().next() },
            None => self.list.head,
        }
    }

    #[inline(always)]
    fn prev_link(&self) -> Link<T> {
        match self.curr {
            Some(curr) => unsafe { T::links(curr).as_ref().prev() },
            None => self.list.tail,
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
    unsafe fn pin_node_mut(&mut self, mut node: NonNull<T>) -> Pin<&mut T> {
        // safety: elements in the list must be pinned while they are in the
        // list, so it is safe to construct a `pin` here provided that the
        // `Linked` trait's invariants are upheld.
        Pin::new_unchecked(node.as_mut())
    }
}

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for CursorMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CursorMut")
            .field("curr", &FmtOption::new(&self.curr))
            .field("list", &self.list)
            .field("index", &self.index)
            .finish()
    }
}
