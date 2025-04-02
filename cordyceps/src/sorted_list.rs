//! [Intrusive], singly-linked, sorted, linked list.
//!
//! See the documentation for the [`SortedList`] and [`SortedListIter`] types for
//! details.
//!
//! [Intrusive]: crate#intrusive-data-structures

use crate::{Linked, Stack};
use core::{
    cmp::{Ord, Ordering},
    fmt,
    marker::PhantomData,
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

    /// Insert a single item into the list, in its sorted order position
    ///
    /// Note that if the inserted item is [`Equal`](Ordering::Equal) to another
    /// item in the list, the newest item is always sorted AFTER the existing
    /// item.
    ///
    /// This is an _O_(_n_) operation.
    pub fn insert(&mut self, element: T::Handle) {
        let eptr = T::into_ptr(element);
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
        let Self { head, func: _ } = self;
        f.debug_struct("SortedList").field("head", head).finish()
    }
}

impl<T: Linked<Links<T>>> Drop for SortedList<T> {
    fn drop(&mut self) {
        // We just turn the list into a stack then run the stack drop code.
        // It already has correct + tested logic for dropping a singly
        // linked list of items one at a time.
        let stack = Stack {
            head: self.head.take(),
        };
        drop(stack);
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
