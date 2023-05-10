use super::{Link, Links, List};
use crate::{util::FmtOption, Linked};
use core::{
    fmt, mem,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::NonNull,
};

/// A cursor over a [`List`] with editing operations.
///
/// A `CursorMut` is like a mutable [`Iterator`] (and it [implements the
/// `Iterator` trait](#impl-Iterator)), except that it can freely seek
/// back and forth, and can  safely mutate the list during iteration. This is
/// because the lifetime of its yielded references is tied to its own lifetime,
/// instead of that of the underlying underlying list. This means cursors cannot
/// yield multiple elements at once.
///
/// Cursors always rest between two elements in the list, and index in a
/// logically circular way &mdash; once a cursor has advanced past the end of
/// the list, advancing it again will "wrap around" to the first element, and
/// seeking past the first element will wrap the cursor around to the end.
///
/// To accommodate this, there is a null non-element that yields `None` between
/// the head and tail of the list. This indicates that the cursor has reached
/// an end of the list.
///
/// This type implements the same interface as the
/// [`alloc::collections::linked_list::CursorMut`] type, and should behave
/// similarly.
pub struct CursorMut<'list, T: Linked<Links<T>> + ?Sized> {
    core: CursorCore<T, &'list mut List<T>>,
}

/// A cursor over a [`List`].
///
/// A `Cursor` is like a by-reference [`Iterator`] (and it [implements the
/// `Iterator` trait](#impl-Iterator)), except that it can freely seek
/// back and forth, and can  safely mutate the list during iteration. This is
/// because the lifetime of its yielded references is tied to its own lifetime,
/// instead of that of the underlying underlying list. This means cursors cannot
/// yield multiple elements at once.
///
/// Cursors always rest between two elements in the list, and index in a
/// logically circular way &mdash; once a cursor has advanced past the end of
/// the list, advancing it again will "wrap around" to the first element, and
/// seeking past the first element will wrap the cursor around to the end.
///
/// To accommodate this, there is a null non-element that yields `None` between
/// the head and tail of the list. This indicates that the cursor has reached
/// an end of the list.
///
/// This type implements the same interface as the
/// [`alloc::collections::linked_list::Cursor`] type, and should behave
/// similarly.
///
/// For a mutable cursor, see the [`CursorMut`] type.
pub struct Cursor<'list, T: Linked<Links<T>> + ?Sized> {
    core: CursorCore<T, &'list List<T>>,
}

/// A type implementing shared functionality between mutable and immutable
/// cursors.
///
/// This allows us to only have a single implementation of methods like
/// `move_next` and `move_prev`, `peek_next,` and `peek_prev`, etc, for both
/// `Cursor` and `CursorMut`.
struct CursorCore<T: ?Sized, L> {
    list: L,
    curr: Link<T>,
    index: usize,
}

// === impl CursorMut ====

impl<'list, T: Linked<Links<T>> + ?Sized> Iterator for CursorMut<'list, T> {
    type Item = Pin<&'list mut T>;
    fn next(&mut self) -> Option<Self::Item> {
        let node = self.core.curr?;
        self.move_next();
        unsafe { Some(self.core.pin_node_mut(node)) }
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

impl<'list, T: Linked<Links<T>> + ?Sized> CursorMut<'list, T> {
    pub(super) fn new(list: &'list mut List<T>, curr: Link<T>, index: usize) -> Self {
        Self {
            core: CursorCore { list, index, curr },
        }
    }

    /// Returns the index of this cursor's position in the [`List`].
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn index(&self) -> Option<usize> {
        self.core.index()
    }

    /// Moves the cursor position to the next element in the [`List`].
    ///
    /// If the cursor is pointing at the null element, this moves it to the first
    /// element in the [`List`]. If it is pointing to the last element in the
    /// list, then this will move it to the null element.
    pub fn move_next(&mut self) {
        self.core.move_next()
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
        self.core.move_prev()
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
        let node = self.core.curr?;
        unsafe {
            // before modifying `node`'s links, set the current element to the
            // one after `node`.
            self.core.curr = T::links(node).as_ref().next();
            // safety: `List::remove` is unsafe to call, because the caller must
            // guarantee that the removed node is part of *that* list. in this
            // case, because the cursor can only access nodes from the list it
            // points to, we know this is safe.
            self.core.list.remove(node)
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
        while !predicate(unsafe { self.core.curr?.as_ref() }) {
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
        self.core.current()
    }

    /// Mutably borrows the element that the cursor is currently pointing at.
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn current_mut(&mut self) -> Option<Pin<&mut T>> {
        self.core
            .curr
            .map(|node| unsafe { self.core.pin_node_mut(node) })
    }

    /// Borrows the next element after the cursor's current position in the
    /// list.
    ///
    /// If the cursor is pointing to the null element, this returns the first
    /// element in the [`List`]. If the cursor is pointing to the last element
    /// in the [`List`], this returns `None`.
    pub fn peek_next(&self) -> Option<Pin<&T>> {
        self.core.peek_next()
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
        self.core.peek_prev()
    }

    /// Mutably borrows the next element after the cursor's current position in
    /// the list.
    ///
    /// If the cursor is pointing to the null element, this returns the first
    /// element in the [`List`]. If the cursor is pointing to the last element
    /// in the [`List`], this returns `None`.
    pub fn peek_next_mut(&mut self) -> Option<Pin<&mut T>> {
        self.core
            .next_link()
            .map(|next| unsafe { self.core.pin_node_mut(next) })
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
        self.core
            .prev_link()
            .map(|prev| unsafe { self.core.pin_node_mut(prev) })
    }

    /// Inserts a new element into the [`List`] after the current one.
    ///
    /// If the cursor is pointing at the null element then the new element is
    /// inserted at the front of the [`List`].
    pub fn insert_after(&mut self, element: T::Handle) {
        let node = T::into_ptr(element);
        assert_ne!(
            self.core.curr,
            Some(node),
            "cannot insert a node after itself"
        );
        let next = self.core.next_link();

        unsafe {
            self.core
                .list
                .insert_nodes_between(self.core.curr, next, node, node, 1);
        }

        if self.core.curr.is_none() {
            // The null index has shifted.
            self.core.index = self.core.list.len;
        }
    }

    /// Inserts a new element into the [`List`] before the current one.
    ///
    /// If the cursor is pointing at the null element then the new element is
    /// inserted at the front of the [`List`].
    pub fn insert_before(&mut self, element: T::Handle) {
        let node = T::into_ptr(element);
        assert_ne!(
            self.core.curr,
            Some(node),
            "cannot insert a node before itself"
        );
        let prev = self.core.prev_link();

        unsafe {
            self.core
                .list
                .insert_nodes_between(prev, self.core.curr, node, node, 1);
        }

        self.core.index += 1;
    }

    /// Returns the length of the [`List`] this cursor points to.
    pub fn len(&self) -> usize {
        self.core.list.len()
    }

    /// Returns `true` if the [`List`] this cursor points to is empty
    pub fn is_empty(&self) -> bool {
        self.core.list.is_empty()
    }

    /// Splits the list into two after the current element. This will return a
    /// new list consisting of everything after the cursor, with the original
    /// list retaining everything before.
    ///
    /// If the cursor is pointing at the null element, then the entire contents
    /// of the `List` are moved.
    pub fn split_after(&mut self) -> List<T> {
        let split_at = if self.core.index == self.core.list.len {
            self.core.index = 0;
            0
        } else {
            self.core.index + 1
        };
        unsafe {
            // safety: we know we are splitting at a node that belongs to our list.
            self.core.list.split_after_node(self.core.curr, split_at)
        }
    }

    /// Splits the list into two before the current element. This will return a
    /// new list consisting of everything before the cursor, with the original
    /// list retaining everything after the cursor.
    ///
    /// If the cursor is pointing at the null element, then the entire contents
    /// of the `List` are moved.
    pub fn split_before(&mut self) -> List<T> {
        let split_at = self.core.index;
        self.core.index = 0;

        // TODO(eliza): this could be rewritten to use `let ... else` when
        // that's supported on `cordyceps`' MSRV.
        let split_node = match self.core.curr {
            Some(node) => node,
            // the split portion is the entire list. just return it.
            None => return mem::replace(self.core.list, List::new()),
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
            self.core.list.head.replace(split_node)
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
        self.core.list.len -= split_at;

        split
    }

    /// Inserts all elements from `spliced` after the cursor's current position.
    ///
    /// If the cursor is pointing at the null element, then the contents of
    /// `spliced` are inserted at the beginning of the `List` the cursor points to.
    pub fn splice_after(&mut self, mut spliced: List<T>) {
        // TODO(eliza): this could be rewritten to use `let ... else` when
        // that's supported on `cordyceps`' MSRV.
        let (splice_head, splice_tail, splice_len) = match spliced.take_all() {
            Some(splice) => splice,
            // the spliced list is empty, do nothing.
            None => return,
        };

        let next = self.core.next_link();
        unsafe {
            // safety: we know `curr` and `next` came from the same list that we
            // are calling `insert_nodes_between` from, because they came from
            // this cursor, which points at `self.list`.
            self.core.list.insert_nodes_between(
                self.core.curr,
                next,
                splice_head,
                splice_tail,
                splice_len,
            );
        }

        if self.core.curr.is_none() {
            self.core.index = self.core.list.len();
        }
    }

    /// Inserts all elements from `spliced` before the cursor's current position.
    ///
    /// If the cursor is pointing at the null element, then the contents of
    /// `spliced` are inserted at the end of the `List` the cursor points to.
    pub fn splice_before(&mut self, mut spliced: List<T>) {
        // TODO(eliza): this could be rewritten to use `let ... else` when
        // that's supported on `cordyceps`' MSRV.
        let (splice_head, splice_tail, splice_len) = match spliced.take_all() {
            Some(splice) => splice,
            // the spliced list is empty, do nothing.
            None => return,
        };

        let prev = self.core.prev_link();
        unsafe {
            // safety: we know `curr` and `prev` came from the same list that we
            // are calling `insert_nodes_between` from, because they came from
            // this cursor, which points at `self.list`.
            self.core.list.insert_nodes_between(
                prev,
                self.core.curr,
                splice_head,
                splice_tail,
                splice_len,
            );
        }

        self.core.index += splice_len;
    }

    /// Returns a read-only cursor pointing to the current element.
    ///
    /// The lifetime of the returned [`Cursor`] is bound to that of the
    /// `CursorMut`, which means it cannot outlive the `CursorMut` and that the
    /// `CursorMut` is frozen for the lifetime of the [`Cursor`].
    #[must_use]
    pub fn as_cursor(&self) -> Cursor<'_, T> {
        Cursor {
            core: CursorCore {
                list: self.core.list,
                curr: self.core.curr,
                index: self.core.index,
            },
        }
    }
}

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for CursorMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            core: CursorCore { list, curr, index },
        } = self;
        f.debug_struct("CursorMut")
            .field("curr", &FmtOption::new(curr))
            .field("list", list)
            .field("index", index)
            .finish()
    }
}

// === impl Cursor ====

impl<'list, T: Linked<Links<T>> + ?Sized> Iterator for Cursor<'list, T> {
    type Item = Pin<&'list T>;
    fn next(&mut self) -> Option<Self::Item> {
        let node = self.core.curr?;
        self.move_next();
        unsafe { Some(self.core.pin_node(node)) }
    }

    /// A [`Cursor`] can never return an accurate `size_hint` --- its lower
    /// bound is always 0 and its upper bound is always `None`.
    ///
    /// This is because the cursor may be moved around within the list through
    /// methods outside of its `Iterator` implementation. This would make any
    /// `size_hint`s a `Cursor`] returns inaccurate.
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> Cursor<'list, T> {
    pub(super) fn new(list: &'list List<T>, curr: Link<T>, index: usize) -> Self {
        Self {
            core: CursorCore { list, index, curr },
        }
    }

    /// Returns the index of this cursor's position in the [`List`].
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn index(&self) -> Option<usize> {
        self.core.index()
    }

    /// Moves the cursor position to the next element in the [`List`].
    ///
    /// If the cursor is pointing at the null element, this moves it to the first
    /// element in the [`List`]. If it is pointing to the last element in the
    /// list, then this will move it to the null element.
    pub fn move_next(&mut self) {
        self.core.move_next();
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
        self.core.move_prev();
    }

    /// Borrows the element that the cursor is currently pointing at.
    ///
    /// This returns `None` if the cursor is currently pointing to the
    /// null element.
    pub fn current(&self) -> Option<Pin<&T>> {
        self.core.current()
    }

    /// Borrows the next element after the cursor's current position in the
    /// list.
    ///
    /// If the cursor is pointing to the null element, this returns the first
    /// element in the [`List`]. If the cursor is pointing to the last element
    /// in the [`List`], this returns `None`.
    pub fn peek_next(&self) -> Option<Pin<&T>> {
        self.core.peek_next()
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
        self.core.peek_prev()
    }

    /// Returns the length of the [`List`] this cursor points to.
    pub fn len(&self) -> usize {
        self.core.list.len()
    }

    /// Returns `true` if the [`List`] this cursor points to is empty
    pub fn is_empty(&self) -> bool {
        self.core.list.is_empty()
    }
}

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for Cursor<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            core: CursorCore { list, curr, index },
        } = self;
        f.debug_struct("Cursor")
            .field("curr", &FmtOption::new(curr))
            .field("list", list)
            .field("index", index)
            .finish()
    }
}

// === impl CursorCore ===

impl<'list, T, L> CursorCore<T, L>
where
    T: Linked<Links<T>> + ?Sized,
    L: Deref<Target = List<T>> + 'list,
{
    fn index(&self) -> Option<usize> {
        self.curr?;
        Some(self.index)
    }

    fn move_next(&mut self) {
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

    fn move_prev(&mut self) {
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

    fn current(&self) -> Option<Pin<&T>> {
        // NOTE(eliza): in this case, we don't *need* to pin the reference,
        // because it's immutable and you can't move out of a shared
        // reference in safe code. but...it makes the API more consistent
        // with `front_mut` etc.
        self.curr.map(|node| unsafe { self.pin_node(node) })
    }

    fn peek_next(&self) -> Option<Pin<&T>> {
        self.next_link().map(|next| unsafe { self.pin_node(next) })
    }

    fn peek_prev(&self) -> Option<Pin<&T>> {
        self.prev_link().map(|prev| unsafe { self.pin_node(prev) })
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
    unsafe fn pin_node(&self, node: NonNull<T>) -> Pin<&'list T> {
        // safety: elements in the list must be pinned while they are in the
        // list, so it is safe to construct a `pin` here provided that the
        // `Linked` trait's invariants are upheld.
        //
        // the lifetime of the returned reference inside the `Pin` is the
        // lifetime of the `CursorMut`'s borrow on the list, so the node ref
        // cannot outlive its referent, provided that `node` actually came from
        // this list (and it would be a violation of this function's safety
        // invariants if it did not).
        Pin::new_unchecked(node.as_ref())
    }
}

impl<'list, T, L> CursorCore<T, L>
where
    T: Linked<Links<T>> + ?Sized,
    L: Deref<Target = List<T>> + DerefMut + 'list,
{
    /// # Safety
    ///
    /// - `node` must point to an element currently in this list.
    unsafe fn pin_node_mut(&self, mut node: NonNull<T>) -> Pin<&'list mut T> {
        // safety: elements in the list must be pinned while they are in the
        // list, so it is safe to construct a `pin` here provided that the
        // `Linked` trait's invariants are upheld.
        //
        // the lifetime of the returned reference inside the `Pin` is the
        // lifetime of the `CursorMut`'s borrow on the list, so the node ref
        // cannot outlive its referent, provided that `node` actually came from
        // this list (and it would be a violation of this function's safety
        // invariants if it did not).
        Pin::new_unchecked(node.as_mut())
    }
}
