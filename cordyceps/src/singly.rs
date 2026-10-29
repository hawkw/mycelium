//! Machinery shared by the singly-linked collections.
//!
//! [`Stack`] and [`SortedList`] are both chains of [`stack::Links`], which hold
//! nothing but a `next` pointer. Anything that has to find an element, rather
//! than take the one at the front, therefore has to walk the chain carrying the
//! element before it, because there is no way back. That walk is the same for
//! both collections, so it lives here rather than twice.
//!
//! [`Stack`]: crate::Stack
//! [`SortedList`]: crate::SortedList
//! [`stack::Links`]: crate::stack::Links
use crate::{stack::Links, Linked};
use core::{fmt, pin::Pin, ptr::NonNull};

/// Returns the element after `ptr`, if there is one.
///
/// # Safety
///
/// `ptr` must point to a live `T` which is linked into a chain of `Links<T>`.
#[inline]
pub(crate) unsafe fn next_of<T>(ptr: NonNull<T>) -> Option<NonNull<T>>
where
    T: Linked<Links<T>>,
{
    T::links(ptr).as_ref().next.with(|next| *next)
}

/// Points `ptr` at `next`, returning whatever it pointed at before.
///
/// # Safety
///
/// `ptr` must point to a live `T`, and the caller must have exclusive access to
/// the chain it belongs to.
#[inline]
pub(crate) unsafe fn set_next<T>(ptr: NonNull<T>, next: Option<NonNull<T>>) -> Option<NonNull<T>>
where
    T: Linked<Links<T>>,
{
    T::links(ptr)
        .as_mut()
        .next
        .with_mut(|slot| core::mem::replace(&mut *slot, next))
}

/// Counts the elements in the chain beginning at `head`.
///
/// This is *O*(*n*); it exists for the places that are handed a chain without a
/// count, and cannot know one any cheaper.
///
/// # Safety
///
/// `head` must be `None` or point to a live `T` beginning a well-formed chain.
pub(crate) unsafe fn count<T>(head: Option<NonNull<T>>) -> usize
where
    T: Linked<Links<T>>,
{
    let mut len = 0;
    let mut curr = head;
    while let Some(node) = curr {
        len += 1;
        curr = next_of(node);
    }
    len
}

/// Unlinks `item` from the chain beginning at `head`, returning the pointer
/// owned by the chain so its provenance is preserved.
///
/// The walk carries the element before the one it is looking at, so that the
/// predecessor can be pointed past `item` once it is found. An element which is
/// not in this chain is simply not found, and the chain is left as it was.
///
/// # Safety
///
/// `head` must begin a well-formed chain the caller has exclusive access to,
/// and `item` must point to a live `T`.
pub(crate) unsafe fn unlink<T>(
    head: &mut Option<NonNull<T>>,
    item: NonNull<T>,
) -> Option<NonNull<T>>
where
    T: Linked<Links<T>>,
{
    let mut prev: Option<NonNull<T>> = None;
    let mut curr = *head;

    while let Some(node) = curr {
        let next = next_of(node);
        if node == item {
            match prev {
                Some(prev) => {
                    set_next(prev, next);
                }
                None => *head = next,
            }
            // an unlinked element carries no link to what used to follow it,
            // so that it can be pushed onto another list straight away.
            set_next(node, None);
            return Some(node);
        }
        prev = Some(node);
        curr = next;
    }

    None
}

/// Drops every element in the chain beginning at `head`, leaving it empty.
///
/// The remaining suffix stays linked and counted if an element's destructor
/// panics.
///
/// # Safety
///
/// `head` must begin a well-formed chain which this caller owns, and `len`
/// must be the number of elements in that chain.
pub(crate) unsafe fn clear<T>(head: &mut Option<NonNull<T>>, len: &mut usize)
where
    T: Linked<Links<T>>,
{
    while let Some(node) = *head {
        // Update the collection before running user code, retaining ownership
        // of every element that has not yet been dropped.
        *head = set_next(node, None);
        *len -= 1;
        drop(T::from_ptr(node));
    }
}

/// An iterator which unlinks and yields the elements a predicate accepts.
///
/// This is the iterator returned by [`Stack::drain_filter`] and
/// [`SortedList::drain_filter`]; see those methods for details.
///
/// [`Stack::drain_filter`]: crate::Stack::drain_filter
/// [`SortedList::drain_filter`]: crate::SortedList::drain_filter
#[must_use = "iterators are lazy, and do nothing unless consumed"]
pub struct DrainFilter<'list, T, F>
where
    T: Linked<Links<T>>,
{
    /// The head of the chain being drained. Held by reference so that removing
    /// the first element updates the collection itself.
    head: &'list mut Option<NonNull<T>>,
    /// The collection's length, kept exact as elements leave.
    len: &'list mut usize,
    /// The last element the predicate rejected, and so the one whose `next`
    /// must be moved past anything removed after it.
    prev: Option<NonNull<T>>,
    /// The next element to hand to the predicate.
    curr: Option<NonNull<T>>,
    pred: F,
}

impl<'list, T, F> DrainFilter<'list, T, F>
where
    T: Linked<Links<T>>,
    F: FnMut(&T) -> bool,
{
    pub(crate) fn new(
        head: &'list mut Option<NonNull<T>>,
        len: &'list mut usize,
        predicate: F,
    ) -> Self {
        let curr = *head;
        Self {
            head,
            len,
            prev: None,
            curr,
            pred: predicate,
        }
    }
}

impl<T, F> Iterator for DrainFilter<'_, T, F>
where
    T: Linked<Links<T>>,
    F: FnMut(&T) -> bool,
{
    type Item = T::Handle;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.curr {
            // Safety: `node` came from the chain this iterator owns, so it is
            // live for as long as the borrow of the collection lasts.
            let next = unsafe { next_of(node) };
            let accepted = (self.pred)(unsafe { node.as_ref() });
            self.curr = next;

            if !accepted {
                // kept: it becomes what anything removed after it links past.
                self.prev = Some(node);
                continue;
            }

            unsafe {
                match self.prev {
                    Some(prev) => {
                        set_next(prev, next);
                    }
                    None => *self.head = next,
                }
                set_next(node, None);
                *self.len -= 1;
                return Some(T::from_ptr(node));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // nothing is known about how many the predicate will accept, but it
        // cannot accept more than the collection still holds.
        (0, Some(*self.len))
    }
}

impl<T, F> fmt::Debug for DrainFilter<'_, T, F>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrainFilter")
            .field("head", self.head)
            .field("len", self.len)
            .field("curr", &self.curr)
            .finish_non_exhaustive()
    }
}

/// The walk shared by both collections' cursors.
///
/// A singly-linked chain has no way back, so unlinking the element the cursor
/// is on needs the one before it. That element is carried here and kept correct
/// as the cursor moves, removes and inserts.
///
/// Like [`List`]'s cursor, this has a position off the list: `current` is
/// `None` there, moving on from it arrives at the front, and it is the position
/// an empty collection's cursor starts in.
///
/// [`List`]: crate::List
pub(crate) struct CursorMut<'list, T: Linked<Links<T>>> {
    /// The collection's head, held by reference so that removing or inserting
    /// at the front updates the collection itself.
    head: &'list mut Option<NonNull<T>>,
    /// The collection's length, kept exact as elements come and go.
    len: &'list mut usize,
    /// The element before `curr`, or `None` when `curr` is the front or the
    /// cursor is off the list.
    prev: Option<NonNull<T>>,
    /// The element the cursor is on, `None` when it is off the list.
    curr: Option<NonNull<T>>,
    /// `curr`'s position from the front; the length when off the list.
    index: usize,
}

impl<'list, T: Linked<Links<T>>> CursorMut<'list, T> {
    pub(crate) fn new(head: &'list mut Option<NonNull<T>>, len: &'list mut usize) -> Self {
        let curr = *head;
        Self {
            head,
            len,
            prev: None,
            curr,
            index: 0,
        }
    }

    pub(crate) fn index(&self) -> Option<usize> {
        self.curr.map(|_| self.index)
    }

    pub(crate) fn current(&self) -> Option<Pin<&T>> {
        // Safety: the collection owns this element and is borrowed for as long
        // as the cursor lives, and an element stays pinned while it is linked.
        self.curr
            .map(|node| unsafe { Pin::new_unchecked(node.as_ref()) })
    }

    pub(crate) fn peek_next(&self) -> Option<Pin<&T>> {
        let next = match self.curr {
            // Safety: `curr` is a live element of this chain.
            Some(node) => unsafe { next_of(node) },
            None => *self.head,
        };
        // Safety: as `current`.
        next.map(|node| unsafe { Pin::new_unchecked(node.as_ref()) })
    }

    pub(crate) fn move_next(&mut self) {
        match self.curr {
            Some(node) => {
                self.prev = Some(node);
                // Safety: `node` is a live element of this chain.
                self.curr = unsafe { next_of(node) };
                self.index += 1;
            }
            None => {
                // off the list: the next element along is the front again.
                self.prev = None;
                self.curr = *self.head;
                self.index = 0;
            }
        }
    }

    pub(crate) fn remove_current(&mut self) -> Option<T::Handle> {
        let node = self.curr?;
        unsafe {
            let next = next_of(node);
            match self.prev {
                Some(prev) => {
                    set_next(prev, next);
                }
                None => *self.head = next,
            }
            // what leaves carries no link back into the chain, so it can go
            // straight into another collection.
            set_next(node, None);
            *self.len -= 1;
            // the cursor is now on whatever followed, at the same position, and
            // the element before it has not changed.
            self.curr = next;
            Some(T::from_ptr(node))
        }
    }

    pub(crate) fn insert_after(&mut self, element: T::Handle) {
        let ptr = T::into_ptr(element);
        unsafe {
            match self.curr {
                Some(node) => {
                    let next = next_of(node);
                    set_next(ptr, next);
                    set_next(node, Some(ptr));
                }
                // off the list, as in `List`, the element goes to the front.
                None => {
                    set_next(ptr, *self.head);
                    *self.head = Some(ptr);
                }
            }
        }
        *self.len += 1;
        if self.curr.is_none() {
            // the off-the-list position has shifted along with the length.
            self.index = *self.len;
        }
    }

    /// Severs the chain after the current element, handing back what came off
    /// and how many elements it holds. Off the list, everything comes off.
    pub(crate) fn split_after(&mut self) -> (Option<NonNull<T>>, usize) {
        match self.curr {
            Some(node) => {
                // Safety: `node` is the last element which stays.
                let rest = unsafe { set_next(node, None) };
                let kept = self.index + 1;
                let moved = *self.len - kept;
                *self.len = kept;
                (rest, moved)
            }
            None => {
                let rest = self.head.take();
                let moved = core::mem::replace(self.len, 0);
                self.prev = None;
                self.index = 0;
                (rest, moved)
            }
        }
    }
}

impl<T: Linked<Links<T>>> fmt::Debug for CursorMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CursorMut")
            .field("index", &self.index)
            .field("curr", &self.curr)
            .field("len", self.len)
            .finish_non_exhaustive()
    }
}
