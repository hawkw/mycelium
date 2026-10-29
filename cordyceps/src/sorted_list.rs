//! [Intrusive], singly-linked, sorted, linked list.
//!
//! See the documentation for the [`SortedList`] and [`SortedListIter`] types for
//! details.
//!
//! [Intrusive]: crate#intrusive-data-structures

use crate::{Linked, Stack};

pub use crate::singly::DrainFilter;

/// A cursor over a [`SortedList`], which walks it in sorted order from the
/// front.
///
/// This is returned by [`SortedList::cursor_front_mut`]; see that method for
/// details. Like [`List`]'s cursor, it has a position off the list, where it
/// holds no element and from which moving on arrives back at the front.
///
/// [`List`]: crate::List
#[derive(Debug)]
pub struct CursorMut<'list, T: Linked<Links<T>>> {
    func: fn(&T, &T) -> Ordering,
    inner: crate::singly::CursorMut<'list, T>,
}

impl<T: Linked<Links<T>>> CursorMut<'_, T> {
    /// Returns how far the cursor is from the front, or `None` when it is off
    /// the list.
    #[must_use]
    pub fn index(&self) -> Option<usize> {
        self.inner.index()
    }

    /// Borrows the element the cursor is on, or `None` when it is off the list.
    #[must_use]
    pub fn current(&self) -> Option<Pin<&T>> {
        self.inner.current()
    }

    /// Borrows the element after the one the cursor is on. Off the list, that
    /// is the front.
    #[must_use]
    pub fn peek_next(&self) -> Option<Pin<&T>> {
        self.inner.peek_next()
    }

    /// Moves the cursor to the next element.
    ///
    /// From the last element this moves off the list; from off the list it
    /// moves to the front, as [`List`]'s cursor does.
    ///
    /// [`List`]: crate::List
    pub fn move_next(&mut self) {
        self.inner.move_next()
    }

    /// Unlinks the element the cursor is on and returns the [`Handle`] owning
    /// it, leaving the cursor on whatever followed.
    ///
    /// Returns `None`, taking nothing, when the cursor is off the list. What
    /// comes back is detached, so it can go straight into another collection.
    ///
    /// [`Handle`]: crate::Linked::Handle
    pub fn remove_current(&mut self) -> Option<T::Handle> {
        self.inner.remove_current()
    }

    /// Splits the list after the element the cursor is on, returning everything
    /// after it as a new list ordered the same way, and leaving that element
    /// and those before it here.
    ///
    /// Off the list, every element is moved.
    #[must_use]
    pub fn split_after(&mut self) -> SortedList<T> {
        let (head, len) = self.inner.split_after();
        SortedList {
            func: self.func,
            head,
            len,
        }
    }
}
use core::{
    cmp::{Ord, Ordering},
    fmt,
    marker::PhantomData,
    pin::Pin,
    ptr::NonNull,
};

pub use crate::stack::Links;

/// A sorted singly linked list
///
/// This behaves similar to [`Stack`], in that it is a singly linked list,
/// however items are stored in an ordered fashion. This means that insertion
/// is an _O_(_n_) operation, and retrieval of the first item is an _O_(1) operation.
///
/// It allows for a user selected ordering operation. If your type `T` implements
/// [`Ord`]:
///
/// * Consider using [`SortedList::new_min()`] if you want **smallest** items sorted first.
/// * Consider using [`SortedList::new_max()`] if you want **largest** items sorted first.
///
/// If your type `T` does NOT implement [`Ord`], or you want to use
/// a custom sorting anyway, consider using [`SortedList::new_with_cmp()`]
///
/// In order to be part of a `SortedList`, a type `T` must implement
/// the [`Linked`] trait for [`sorted_list::Links<T>`](Links), which is an alias for
/// [`stack::Links<T>`](Links). This means that you can link the same element into
/// either structure, but you can't have something that's linked into a `SortedList`
/// and a `Stack` at the same time (without wrapper structs that have separate sets
/// of links, left as an exercise for the reader).
///
/// Pushing elements into a `SortedList` takes ownership of those elements
/// through an owning [`Handle` type](Linked::Handle). Dropping a
/// `SortedList` drops all elements currently linked into the stack.
pub struct SortedList<T: Linked<Links<T>>> {
    head: Option<NonNull<T>>,
    /// The number of elements linked into this list.
    len: usize,
    // Returns if LHS is less/same/greater than RHS
    func: fn(&T, &T) -> Ordering,
}

#[inline]
fn invert_sort<T: Ord>(a: &T, b: &T) -> Ordering {
    // Inverted sort order!
    T::cmp(b, a)
}

impl<T> SortedList<T>
where
    T: Linked<Links<T>>,
    T: Ord,
{
    /// Create a new (empty) sorted list, sorted LEAST FIRST
    ///
    /// * Consider using [`SortedList::new_max()`] if you want **largest** items sorted first.
    /// * Consider using [`SortedList::new_with_cmp()`] if you want to provide your own sorting
    ///   implementation.
    ///
    /// If two items are considered of equal value, new values will be placed AFTER
    /// old values.
    #[must_use]
    pub const fn new_min() -> Self {
        Self::new_with_cmp(T::cmp)
    }

    /// Create a new sorted list, consuming the stack, sorted LEAST FIRST
    #[must_use]
    pub fn from_stack_min(stack: Stack<T>) -> Self {
        Self::from_stack_with_cmp(stack, T::cmp)
    }

    /// Create a new (empty) sorted list, sorted GREATEST FIRST
    ///
    /// * Consider using [`SortedList::new_min()`] if you want **smallest** items sorted first.
    /// * Consider using [`SortedList::new_with_cmp()`] if you want to provide your own sorting
    ///   implementation.
    ///
    /// If two items are considered of equal value, new values will be placed AFTER
    /// old values.
    #[must_use]
    pub const fn new_max() -> Self {
        Self::new_with_cmp(invert_sort::<T>)
    }

    /// Create a new sorted list, consuming the stack, sorted GREATEST FIRST
    #[must_use]
    pub fn from_stack_max(stack: Stack<T>) -> Self {
        Self::from_stack_with_cmp(stack, invert_sort::<T>)
    }
}

impl<T: Linked<Links<T>>> SortedList<T> {
    /// Create a new (empty) sorted list with the given ordering function
    ///
    /// If your type T implements [`Ord`]:
    ///
    /// * Consider using [`SortedList::new_min()`] if you want **smallest** items sorted first.
    /// * Consider using [`SortedList::new_max()`] if you want **largest** items sorted first.
    ///
    /// If `T` contained an `i32`, and you wanted the SMALLEST items at the
    /// front, then you could provide a function something like:
    ///
    /// ```rust
    /// let f: fn(&i32, &i32) -> core::cmp::Ordering = |lhs, rhs| {
    ///     lhs.cmp(rhs)
    /// };
    /// ```
    ///
    /// SortedList takes a function (and not just [Ord]) so you can use it on types
    /// that don't have a general way of ordering them, allowing you to select a
    /// specific metric within the sorting function.
    ///
    /// If two items are considered of equal value, new values will be placed AFTER
    /// old values.
    pub const fn new_with_cmp(f: fn(&T, &T) -> Ordering) -> Self {
        Self {
            func: f,
            head: None,
            len: 0,
        }
    }

    /// Create a new sorted list, consuming the stack, using the provided ordering function
    pub fn from_stack_with_cmp(stack: Stack<T>, f: fn(&T, &T) -> Ordering) -> Self {
        let mut slist = Self::new_with_cmp(f);
        slist.extend(stack);
        slist
    }

    /// Pop the front-most item from the list, returning it by ownership (if it exists)
    ///
    /// Note that "front" here refers to the sorted ordering. If this list was created
    /// with [`SortedList::new_min`], the SMALLEST item will be popped. If this was
    /// created with [`SortedList::new_max`], the LARGEST item will be popped.
    ///
    /// This is an _O_(1) operation.
    #[must_use]
    pub fn pop_front(&mut self) -> Option<T::Handle> {
        test_trace!(?self.head, "SortedList::pop_front");
        let head = self.head.take()?;
        self.len -= 1;
        unsafe {
            // Safety: we have exclusive ownership over this chunk of stack.

            // advance the head link to the next node after the current one (if
            // there is one).
            self.head = T::links(head).as_mut().next.with_mut(|next| (*next).take());

            test_trace!(?self.head, "SortedList::pop -> popped");

            // return the current node
            Some(T::from_ptr(head))
        }
    }

    /// Returns the number of elements in this list.
    ///
    /// This is an *O*(1) operation.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if this list contains no elements.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Borrows the element [`pop_front`](Self::pop_front) would return, without
    /// unlinking it, or `None` if the list is empty.
    #[must_use]
    pub fn front(&self) -> Option<Pin<&T>> {
        let head = self.head?;
        // Safety: the list owns this element, so it lives at least as long as
        // the borrow of `self`, and it is pinned for as long as it is linked.
        unsafe { Some(Pin::new_unchecked(head.as_ref())) }
    }

    /// Drops every element still linked into this list, leaving it empty.
    ///
    /// If an element's destructor panics, the remaining elements stay linked
    /// and the length continues to describe them.
    pub fn clear(&mut self) {
        // Safety: this list owns every element in its chain and counts them.
        unsafe { crate::singly::clear(&mut self.head, &mut self.len) }
    }

    /// Unlinks `item` from this list, returning the [`Handle`] that owns it.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(T::Handle)` owning `item`, if `item` was linked into this
    ///   list.
    /// - [`None`] if `item` is not an element of this list, in which case the
    ///   list is left exactly as it was.
    ///
    /// Because the list is singly linked, this walks it to find the element
    /// before `item`, and is therefore *O*(*n*).
    ///
    /// [`Handle`]: crate::Linked::Handle
    ///
    /// # Safety
    ///
    /// `item` must point to an element which has not been freed.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Handle> {
        if let Some(node) = crate::singly::unlink(&mut self.head, item) {
            self.len -= 1;
            Some(T::from_ptr(node))
        } else {
            None
        }
    }

    /// Returns an iterator which unlinks and yields the elements `pred`
    /// accepts, leaving the rest linked and still in sorted order.
    ///
    /// Elements are handed to `pred` in sorted order. What the iterator yields
    /// is owned by the caller; dropping the iterator before it is exhausted
    /// leaves everything it has not reached linked, and the length correct.
    pub fn drain_filter<F>(&mut self, pred: F) -> DrainFilter<'_, T, F>
    where
        F: FnMut(&T) -> bool,
    {
        DrainFilter::new(&mut self.head, &mut self.len, pred)
    }

    /// Returns a cursor over this list, starting at the front.
    ///
    /// The cursor walks forwards only, which is all a singly-linked chain
    /// allows, and can look at the element it is on, take it out, or split the
    /// list there. It cannot put an element in at an arbitrary place, which
    /// would not keep the list sorted; that is what [`insert`] is for.
    ///
    /// [`insert`]: Self::insert
    #[must_use]
    pub fn cursor_front_mut(&mut self) -> CursorMut<'_, T> {
        CursorMut {
            func: self.func,
            inner: crate::singly::CursorMut::new(&mut self.head, &mut self.len),
        }
    }

    /// Moves every element of `other` into this list, leaving `other` empty.
    ///
    /// The result is ordered by *this* list's ordering function; the one
    /// `other` was built with has no bearing on where its elements land, and
    /// `other` may well be ordered in a way this list is not. Where an element
    /// of each compares [`Equal`](Ordering::Equal), this list's comes first,
    /// which is the order [`insert`](Self::insert) also gives.
    ///
    /// Each element of `other` is placed in its sorted position here, so this
    /// is *O*(*n* * *m*) in the worst case.
    pub fn merge(&mut self, other: &mut Self) {
        // Taken one at a time, so that `other` still owns everything not yet
        // moved: the ordering function belongs to the caller and may panic, and
        // neither list may lose an element or miscount if it does.
        //
        // `insert` places an element after any it compares equal to, so the
        // elements already here keep their place ahead of `other`'s.
        while let Some(element) = other.pop_front() {
            self.insert(element);
        }
    }

    /// Splits the list in two at `at`, leaving the first `at` elements here and
    /// returning the rest as a new list ordered the same way.
    ///
    /// Splitting at zero moves every element into the returned list; splitting
    /// at the length returns an empty list.
    ///
    /// # Panics
    ///
    /// If `at > self.len()`.
    #[must_use]
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(
            at <= self.len,
            "cannot split at index {at}: the list is only {} elements long",
            self.len
        );

        let mut split = Self {
            func: self.func,
            head: None,
            len: self.len - at,
        };

        if at == 0 {
            split.head = self.head.take();
            self.len = 0;
            return split;
        }

        // Walk to the last element which stays, then sever after it.
        let mut cursor = self.head.expect("at > 0, so the list is not empty");
        for _ in 1..at {
            // Safety: `at <= len`, so there are at least `at` elements to walk.
            cursor = unsafe { crate::singly::next_of(cursor) }
                .expect("the list holds at least `at` elements");
        }
        // Safety: `cursor` is the last element which stays in this list.
        split.head = unsafe { crate::singly::set_next(cursor, None) };
        self.len = at;
        split
    }

    /// Splits the list in two at `at`, leaving the first `at` elements here and
    /// returning the rest as a new list ordered the same way, or `None` if `at`
    /// is past the end of the list.
    ///
    /// This is the non-panicking form of [`split_off`](Self::split_off).
    #[must_use]
    pub fn try_split_off(&mut self, at: usize) -> Option<Self> {
        if at > self.len {
            return None;
        }
        Some(self.split_off(at))
    }

    /// Asserts that this list holds together: that its length is the number of
    /// elements actually linked into it, and that they are in sorted order.
    ///
    /// This is intended for tests and debug assertions.
    ///
    /// # Panics
    ///
    /// If the list's length disagrees with its contents, or an element is out
    /// of order.
    #[track_caller]
    pub fn assert_valid(&self) {
        let mut walked = 0;
        let mut curr = self.head;
        let mut prev: Option<NonNull<T>> = None;

        while let Some(node) = curr {
            walked += 1;
            assert!(
                walked <= self.len,
                "list holds more than the {} elements it claims; it may be cyclic",
                self.len
            );
            if let Some(prev) = prev {
                // Safety: both are live elements of this list's chain.
                let ord = unsafe { (self.func)(prev.as_ref(), node.as_ref()) };
                assert_ne!(
                    ord,
                    Ordering::Greater,
                    "element {walked} of the list sorts before the one in front of it"
                );
            }
            prev = Some(node);
            // Safety: `node` is a live element of this list's chain.
            curr = unsafe { crate::singly::next_of(node) };
        }

        assert_eq!(
            walked, self.len,
            "list claims {} elements but {walked} are linked into it",
            self.len
        );
    }

    /// Insert a single item into the list, in its sorted order position
    ///
    /// Note that if the inserted item is [`Equal`](Ordering::Equal) to another
    /// item in the list, the newest item is always sorted AFTER the existing
    /// item.
    ///
    /// This is an _O_(_n_) operation.
    pub fn insert(&mut self, element: T::Handle) {
        let eptr = T::into_ptr(element);
        // The ordering function is supplied by the caller and may panic. Until
        // the element is linked nothing owns it, so hold it here: if `link`
        // unwinds, this drops the element rather than stranding it, and the
        // length is not touched until the element really is in the list.
        let mut pending = Pending::<T>(Some(eptr));
        // Safety: `eptr` came from a `Handle`, so it is live and unlinked.
        unsafe { self.link(eptr) };
        pending.0 = None;
        self.len += 1;
    }

    /// Links `eptr` into its sorted position. Does not touch the length.
    ///
    /// # Safety
    ///
    /// `eptr` must own a live `T` which is not linked into any list.
    unsafe fn link(&mut self, eptr: NonNull<T>) {
        test_trace!(?eptr, ?self.head, "SortedList::insert");
        debug_assert!(
            unsafe { T::links(eptr).as_ref().next.with(|n| (*n).is_none()) },
            "Inserted items should not already be part of a list"
        );

        // Take a long-lived reference to the new element
        let eref = unsafe { eptr.as_ref() };

        // Special case for empty head
        //
        // If the head is null, then just place the item
        let Some(mut cursor) = self.head else {
            self.head = Some(eptr);
            return;
        };

        // Special case for head: do we replace current head with new element?
        {
            // compare, but make sure we drop the live reference to the cursor
            // so to be extra sure about NOT violating provenance, when we
            // potentially mutate the cursor below, and we really don't want
            // a shared reference to be live.
            let cmp = {
                let cref = unsafe { cursor.as_ref() };
                (self.func)(cref, eref)
            };

            // If cursor node is LESS or EQUAL: keep moving.
            // If cursor node is GREATER: we need to place the new item BEFORE
            if cmp == Ordering::Greater {
                unsafe {
                    let links = T::links(eptr).as_mut();
                    links.next.with_mut(|next| {
                        *next = self.head.replace(eptr);
                    });
                    return;
                }
            }
        }

        // On every iteration of the loop, we know that the new element should
        // be placed AFTER the current value of `cursor`, meaning that we need
        // to decide whether we:
        //
        // * just append (if next is null)
        // * insert between cursor and cursor.next (if elem is < c.next)
        // * just iterate (if elem >= c.next)
        loop {
            // Safety: We have exclusive access to the list, we are allowed to
            // access and mutate it (carefully)
            unsafe {
                // Get the cursor's links
                let clinks = T::links(cursor).as_mut();
                // Peek into the cursor's next item
                let next = clinks.next.with_mut(|next| {
                    // We can take a reference here, as we have exclusive access
                    let mutref = &mut *next;

                    if let Some(n) = mutref {
                        // If next is some, store this pointer
                        let nptr: NonNull<T> = *n;
                        // Then compare the next element with the new element
                        let cmp = {
                            let nref: &T = nptr.as_ref();
                            (self.func)(nref, eref)
                        };

                        if cmp == Ordering::Greater {
                            // As above, if cursor.next > element, then we
                            // need to insert between cursor and next.
                            //
                            // First, get the current element's links...
                            let elinks = T::links(eptr).as_mut();
                            // ...then store cursor.next.next in element.next,
                            // and store element in cursor.next.
                            elinks.next.with_mut(|enext| {
                                *enext = mutref.replace(eptr);
                            });
                            // If we have placed element, there is no next value
                            // for cursor.
                            None
                        } else {
                            // If cursor.next <= element, then we just need to
                            // iterate, so return the NonNull that represents
                            // cursor.next, so we can move cursor there.
                            Some(nptr)
                        }
                    } else {
                        // "just append" case - assign element to cursor.next
                        *mutref = Some(eptr);
                        // If we have placed element, there is no next value
                        // for cursor
                        None
                    }
                });

                // We do the assignment through this tricky return to ensure that the
                // mutable reference to cursor.next we held as "mutref" above has been
                // dropped, so we are not mutating `cursor` while a reference derived
                // from it's provenance is live.
                //
                // We also can't early return inside the loop because all of the body
                // is inside a closure.
                //
                // This might be overly cautious, refactor carefully (with miri).
                let Some(n) = next else {
                    // We're done, return
                    return;
                };
                cursor = n;
            }
        }
    }

    /// Iterate through the items of the list, in sorted order
    pub fn iter(&self) -> SortedListIter<'_, T> {
        SortedListIter {
            _plt: PhantomData,
            node: self.head,
        }
    }
}

impl<T: Linked<Links<T>>> Extend<T::Handle> for SortedList<T> {
    fn extend<I: IntoIterator<Item = T::Handle>>(&mut self, iter: I) {
        for elem in iter {
            self.insert(elem);
        }
    }
}

impl<T> fmt::Debug for SortedList<T>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { head, func: _, len } = self;
        f.debug_struct("SortedList")
            .field("head", head)
            .field("len", len)
            .finish()
    }
}

impl<T: Linked<Links<T>>> Drop for SortedList<T> {
    fn drop(&mut self) {
        // We just turn the list into a stack then run the stack drop code.
        // It already has correct + tested logic for dropping a singly
        // linked list of items one at a time.
        let stack = Stack {
            head: self.head.take(),
            len: core::mem::take(&mut self.len),
        };
        drop(stack);
    }
}

/// Owns an element which is on its way into a list.
///
/// Dropping this drops the element, so an insertion which unwinds part way
/// through does not leave it owned by nobody.
struct Pending<T: Linked<Links<T>>>(Option<NonNull<T>>);

impl<T: Linked<Links<T>>> Drop for Pending<T> {
    fn drop(&mut self) {
        if let Some(ptr) = self.0.take() {
            // Safety: this type is only ever given a pointer which came from a
            // `Handle` and has not been linked into a list.
            drop(unsafe { T::from_ptr(ptr) });
        }
    }
}

/// A borrowing iterator of a [`SortedList`]
pub struct SortedListIter<'a, T: Linked<Links<T>>> {
    _plt: PhantomData<&'a SortedList<T>>,
    node: Option<NonNull<T>>,
}

impl<'a, T: Linked<Links<T>>> Iterator for SortedListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let nn = self.node.take()?;
        unsafe {
            // Advance our pointer to next
            let links = T::links(nn).as_ref();
            self.node = links.next.with(|t| *t);
            Some(nn.as_ref())
        }
    }
}

#[cfg(test)]
mod loom {
    use super::*;
    use crate::loom;
    use crate::stack::test_util::Entry;

    #[test]
    fn builtin_sort_min() {
        loom::model(|| {
            let mut slist = SortedList::<Entry>::new_min();
            // Insert out of order
            slist.insert(Entry::new(20));
            slist.insert(Entry::new(10));
            slist.insert(Entry::new(30));
            slist.insert(Entry::new(25));
            slist.insert(Entry::new(35));
            slist.insert(Entry::new(1));
            slist.insert(Entry::new(2));
            slist.insert(Entry::new(3));
            // expected is in order
            let expected = [1, 2, 3, 10, 20, 25, 30, 35];

            // Does iteration work (twice)?
            {
                let mut ct = 0;
                let siter = slist.iter();
                for (l, r) in expected.iter().zip(siter) {
                    ct += 1;
                    assert_eq!(*l, r.val);
                }
                assert_eq!(ct, expected.len());
            }
            {
                let mut ct = 0;
                let siter = slist.iter();
                for (l, r) in expected.iter().zip(siter) {
                    ct += 1;
                    assert_eq!(*l, r.val);
                }
                assert_eq!(ct, expected.len());
            }

            // Does draining work (once)?
            {
                let mut ct = 0;
                for exp in expected.iter() {
                    let act = slist.pop_front().unwrap();
                    ct += 1;
                    assert_eq!(*exp, act.val);
                }
                assert_eq!(ct, expected.len());
                assert!(slist.pop_front().is_none());
            }
        })
    }

    #[test]
    fn builtin_sort_max() {
        loom::model(|| {
            let mut slist = SortedList::<Entry>::new_max();
            // Insert out of order
            slist.insert(Entry::new(20));
            slist.insert(Entry::new(10));
            slist.insert(Entry::new(30));
            slist.insert(Entry::new(25));
            slist.insert(Entry::new(35));
            slist.insert(Entry::new(1));
            slist.insert(Entry::new(2));
            slist.insert(Entry::new(3));
            // expected is in order (reverse!)
            let expected = [35, 30, 25, 20, 10, 3, 2, 1];

            // Does iteration work (twice)?
            {
                let mut ct = 0;
                let siter = slist.iter();
                for (l, r) in expected.iter().zip(siter) {
                    ct += 1;
                    assert_eq!(*l, r.val);
                }
                assert_eq!(ct, expected.len());
            }
            {
                let mut ct = 0;
                let siter = slist.iter();
                for (l, r) in expected.iter().zip(siter) {
                    ct += 1;
                    assert_eq!(*l, r.val);
                }
                assert_eq!(ct, expected.len());
            }

            // Does draining work (once)?
            {
                let mut ct = 0;
                for exp in expected.iter() {
                    let act = slist.pop_front().unwrap();
                    ct += 1;
                    assert_eq!(*exp, act.val);
                }
                assert_eq!(ct, expected.len());
                assert!(slist.pop_front().is_none());
            }
        })
    }

    #[test]
    fn slist_basic() {
        loom::model(|| {
            let mut slist = SortedList::<Entry>::new_with_cmp(|lhs, rhs| lhs.val.cmp(&rhs.val));
            // Insert out of order
            slist.insert(Entry::new(20));
            slist.insert(Entry::new(10));
            slist.insert(Entry::new(30));
            slist.insert(Entry::new(25));
            slist.insert(Entry::new(35));
            slist.insert(Entry::new(1));
            slist.insert(Entry::new(2));
            slist.insert(Entry::new(3));
            // expected is in order
            let expected = [1, 2, 3, 10, 20, 25, 30, 35];

            // Does iteration work (twice)?
            {
                let mut ct = 0;
                let siter = slist.iter();
                for (l, r) in expected.iter().zip(siter) {
                    ct += 1;
                    assert_eq!(*l, r.val);
                }
                assert_eq!(ct, expected.len());
            }
            {
                let mut ct = 0;
                let siter = slist.iter();
                for (l, r) in expected.iter().zip(siter) {
                    ct += 1;
                    assert_eq!(*l, r.val);
                }
                assert_eq!(ct, expected.len());
            }

            // Does draining work (once)?
            {
                let mut ct = 0;
                for exp in expected.iter() {
                    let act = slist.pop_front().unwrap();
                    ct += 1;
                    assert_eq!(*exp, act.val);
                }
                assert_eq!(ct, expected.len());
                assert!(slist.pop_front().is_none());
            }

            // Do a little pop and remove dance to make sure we correctly unset
            // next on popped items (there is a debug assert for this)
            slist.insert(Entry::new(20));
            slist.insert(Entry::new(10));
            slist.insert(Entry::new(30));
            let x = slist.pop_front().unwrap();
            slist.insert(x);
            let y = slist.pop_front().unwrap();
            let z = slist.pop_front().unwrap();
            slist.insert(y);
            slist.insert(z);
        })
    }
}
