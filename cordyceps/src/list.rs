//! An intrusive doubly-linked list.
//!
//! See the [`List`] type for details.

use super::Linked;
use crate::util::FmtOption;
use core::{
    cell::UnsafeCell,
    fmt,
    marker::PhantomPinned,
    mem,
    pin::Pin,
    ptr::{self, NonNull},
};

#[cfg(test)]
#[cfg(not(loom))]
mod tests;

/// An [intrusive] doubly-linked list.
///
/// This data structure may be used as a first-in, first-out queue by using the
/// [`List::push_front`] and [`List::pop_back`] methods. It also supports
/// random-access removals using the [`List::remove`] method.
///
/// This data structure can also be used as a stack or doubly-linked list by using
/// the [`List::pop_front`] and [`List::push_back`] methods.
///
/// In order to be part of a `List`, a type `T` must implement [`Linked`] for
/// [`list::Links<T>`].
///
/// # Examples
///
/// Implementing the [`Linked`] trait for an entry type:
///
/// ```
/// use cordyceps::{
///     Linked,
///     list::{self, List},
/// };
///
/// // This example uses the Rust standard library for convenience, but
/// // the doubly-linked list itself does not require std.
/// use std::{pin::Pin, ptr::NonNull, thread, sync::Arc};
///
/// /// A simple queue entry that stores an `i32`.
/// // This type must be `repr(C)` in order for the cast in `Linked::links`
/// // to be sound.
/// #[repr(C)]
/// #[derive(Debug, Default)]
/// struct Entry {
///    links: list::Links<Entry>,
///    val: i32,
/// }
///
/// // Implement the `Linked` trait for our entry type so that it can be used
/// // as a queue entry.
/// unsafe impl Linked<list::Links<Entry>> for Entry {
///     // In this example, our entries will be "owned" by a `Box`, but any
///     // heap-allocated type that owns an element may be used.
///     //
///     // An element *must not* move while part of an intrusive data
///     // structure. In many cases, `Pin` may be used to enforce this.
///     type Handle = Pin<Box<Self>>;
///
///     /// Convert an owned `Handle` into a raw pointer
///     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
///        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
///     }
///
///     /// Convert a raw pointer back into an owned `Handle`.
///     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
///         // Safety: if this function is only called by the linked list
///         // implementation (and it is not intended for external use), we can
///         // expect that the `NonNull` was constructed from a reference which
///         // was pinned.
///         //
///         // If other callers besides `List`'s internals were to call this on
///         // some random `NonNull<Entry>`, this would not be the case, and
///         // this could be constructing an erroneous `Pin` from a referent
///         // that may not be pinned!
///         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
///     }
///
///     /// Access an element's `Links`.
///     unsafe fn links(target: NonNull<Entry>) -> NonNull<list::Links<Entry>> {
///         // Safety: this cast is safe only because `Entry` `is repr(C)` and
///         // the links is the first field.
///         target.cast()
///     }
/// }
///
/// impl Entry {
///     fn new(val: i32) -> Self {
///         Self {
///             val,
///             ..Self::default()
///         }
///     }
/// }
/// ```
///
/// Using a `List` as a first-in, first-out (FIFO) queue with
/// [`List::push_back`] and [`List::pop_front`]:
/// ```
/// # use cordyceps::{
/// #     Linked,
/// #     list::{self, List},
/// # };
/// # use std::{pin::Pin, ptr::NonNull, thread, sync::Arc};
/// # #[repr(C)]
/// # #[derive(Debug, Default)]
/// # struct Entry {
/// #    links: list::Links<Entry>,
/// #    val: i32,
/// # }
/// # unsafe impl Linked<list::Links<Entry>> for Entry {
/// #     type Handle = Pin<Box<Self>>;
/// #     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
/// #        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
/// #     }
/// #     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
/// #         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
/// #     }
/// #     unsafe fn links(target: NonNull<Entry>) -> NonNull<list::Links<Entry>> {
/// #         target.cast()
/// #     }
/// # }
/// # impl Entry {
/// #     fn new(val: i32) -> Self {
/// #         Self {
/// #             val,
/// #             ..Self::default()
/// #         }
/// #     }
/// # }
/// // Now that we've implemented the `Linked` trait for our `Entry` type, we can
/// // create a `List` of entries:
/// let mut list = List::<Entry>::new();
///
/// // Push some entries to the list:
/// for i in 0..5 {
///     list.push_back(Box::pin(Entry::new(i)));
/// }
///
/// // The list is a doubly-ended queue. We can use the `pop_front` method with
/// // `push_back` to dequeue elements in FIFO order:
/// for i in 0..5 {
///     let entry = list.pop_front()
///         .expect("the list should have 5 entries in it");
///     assert_eq!(entry.val, i, "entries are dequeued in FIFO order");
/// }
///
/// assert!(list.is_empty());
/// ```
///
/// Using a `List` as a last-in, first-out (LIFO) stack with
/// [`List::push_back`] and [`List::pop_back`]:
/// ```
/// # use cordyceps::{
/// #     Linked,
/// #     list::{self, List},
/// # };
/// # use std::{pin::Pin, ptr::NonNull, thread, sync::Arc};
/// # #[repr(C)]
/// # #[derive(Debug, Default)]
/// # struct Entry {
/// #    links: list::Links<Entry>,
/// #    val: i32,
/// # }
/// # unsafe impl Linked<list::Links<Entry>> for Entry {
/// #     type Handle = Pin<Box<Self>>;
/// #     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
/// #        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
/// #     }
/// #     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
/// #         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
/// #     }
/// #     unsafe fn links(target: NonNull<Entry>) -> NonNull<list::Links<Entry>> {
/// #         target.cast()
/// #     }
/// # }
/// # impl Entry {
/// #     fn new(val: i32) -> Self {
/// #         Self {
/// #             val,
/// #             ..Self::default()
/// #         }
/// #     }
/// # }
/// let mut list = List::<Entry>::new();
///
/// // Push some entries to the list:
/// for i in 0..5 {
///     list.push_back(Box::pin(Entry::new(i)));
/// }
///
/// // Note that we have reversed the direction of the iterator, since
/// // we are popping from the *back* of the list:
/// for i in (0..5).into_iter().rev() {
///     let entry = list.pop_back()
///         .expect("the list should have 5 entries in it");
///     assert_eq!(entry.val, i, "entries are dequeued in LIFO order");
/// }
///
/// assert!(list.is_empty());
/// ```
///
/// [intrusive]: crate#intrusive-data-structures
/// [`list::Links<T>`]: crate::list::Links
pub struct List<T: Linked<Links<T>> + ?Sized> {
    head: Link<T>,
    tail: Link<T>,
    len: usize,
}

/// Links to other nodes in a [`List`].
///
/// In order to be part of a [`List`], a type must contain an instance of this
/// type, and must implement the [`Linked`] trait for `Links<Self>`.
pub struct Links<T: ?Sized> {
    inner: UnsafeCell<LinksInner<T>>,
}

/// A cursor over a [`List`].
///
/// This is similar to a mutable iterator (and implements the [`Iterator`]
/// trait), but it also permits modification to the list itself.
pub struct Cursor<'list, T: Linked<Links<T>> + ?Sized> {
    list: &'list mut List<T>,
    curr: Link<T>,
    len: usize,
}

/// Iterates over the items in a [`List`] by reference.
pub struct Iter<'list, T: Linked<Links<T>> + ?Sized> {
    _list: &'list List<T>,

    /// The current node when iterating head -> tail.
    curr: Link<T>,

    /// The current node when iterating tail -> head.
    ///
    /// This is used by the [`DoubleEndedIterator`] impl.
    curr_back: Link<T>,

    /// The number of remaining entries in the iterator.
    len: usize,
}

/// Iterates over the items in a [`List`] by mutable reference.
pub struct IterMut<'list, T: Linked<Links<T>> + ?Sized> {
    _list: &'list mut List<T>,

    /// The current node when iterating head -> tail.
    curr: Link<T>,

    /// The current node when iterating tail -> head.
    ///
    /// This is used by the [`DoubleEndedIterator`] impl.
    curr_back: Link<T>,

    /// The number of remaining entries in the iterator.
    len: usize,
}

/// An iterator returned by [`List::drain_filter`].
pub struct DrainFilter<'list, T, F>
where
    F: FnMut(&T) -> bool,
    T: Linked<Links<T>> + ?Sized,
{
    list: &'list mut List<T>,
    curr: Link<T>,
    pred: F,
    seen: usize,
    old_len: usize,
}

type Link<T> = Option<NonNull<T>>;

#[repr(C)]
struct LinksInner<T: ?Sized> {
    next: Link<T>,
    prev: Link<T>,
    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// <https://github.com/rust-lang/rust/issues/63818>.
    _unpin: PhantomPinned,
}

// ==== impl List ====

impl<T: Linked<Links<T>> + ?Sized> List<T> {
    /// Returns a new empty list.
    #[must_use]
    pub const fn new() -> List<T> {
        List {
            head: None,
            tail: None,
            len: 0,
        }
    }

    /// Returns `true` if this list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        if self.head.is_none() {
            debug_assert!(
                self.tail.is_none(),
                "inconsistent state: a list had a tail but no head!"
            );
            debug_assert_eq!(
                self.len, 0,
                "inconsistent state: a list was empty, but its length was not zero"
            );
            return true;
        }

        debug_assert_ne!(
            self.len, 0,
            "inconsistent state: a list was not empty, but its length was zero"
        );
        false
    }

    /// Returns the number of elements in the list.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Asserts as many of the linked list's invariants as possible.
    pub fn assert_valid(&self) {
        let head = match self.head {
            Some(head) => head,
            None => {
                assert!(
                    self.tail.is_none(),
                    "if the linked list's head is null, the tail must also be null"
                );
                assert_eq!(
                    self.len, 0,
                    "if a linked list's head is null, its length must be 0"
                );
                return;
            }
        };

        assert_ne!(
            self.len, 0,
            "if a linked list's head is not null, its length must be greater than 0"
        );

        let tail = self
            .tail
            .expect("if the linked list has a head, it must also have a tail");
        let head_links = unsafe { T::links(head) };
        let tail_links = unsafe { T::links(tail) };
        let head_links = unsafe { head_links.as_ref() };
        let tail_links = unsafe { tail_links.as_ref() };
        if head == tail {
            assert_eq!(
                head_links, tail_links,
                "if the head and tail nodes are the same, their links must be the same"
            );
            assert_eq!(
                head_links.next(),
                None,
                "if the linked list has only one node, it must not be linked"
            );
            assert_eq!(
                head_links.prev(),
                None,
                "if the linked list has only one node, it must not be linked"
            );
            return;
        }

        let mut curr = Some(head);
        let mut actual_len = 0;
        while let Some(node) = curr {
            let links = unsafe { T::links(node) };
            let links = unsafe { links.as_ref() };
            links.assert_valid(head_links, tail_links);
            curr = links.next();
            actual_len += 1;
        }

        assert_eq!(self.len, actual_len);
    }

    /// Appends an item to the head of the list.
    pub fn push_front(&mut self, item: T::Handle) {
        let ptr = T::into_ptr(item);
        // tracing::trace!(?self, ?ptr, "push_front");
        assert_ne!(self.head, Some(ptr));
        unsafe {
            T::links(ptr).as_mut().set_next(self.head);
            T::links(ptr).as_mut().set_prev(None);
            // tracing::trace!(?links);
            if let Some(head) = self.head {
                T::links(head).as_mut().set_prev(Some(ptr));
                // tracing::trace!(head.links = ?T::links(head).as_ref(), "set head prev ptr",);
            }
        }

        self.head = Some(ptr);

        if self.tail.is_none() {
            self.tail = Some(ptr);
        }

        self.len += 1;
        // tracing::trace!(?self, "push_front: pushed");
    }

    /// Appends an item to the tail of the list
    pub fn push_back(&mut self, item: T::Handle) {
        let ptr = T::into_ptr(item);
        assert_ne!(self.tail, Some(ptr));
        unsafe {
            T::links(ptr).as_mut().set_next(None);
            T::links(ptr).as_mut().set_prev(self.tail);
            if let Some(tail) = self.tail {
                T::links(tail).as_mut().set_next(Some(ptr));
            }
        }

        self.tail = Some(ptr);
        if self.head.is_none() {
            self.head = Some(ptr);
        }

        self.len += 1;
    }

    /// Remove an item from the head of the list
    pub fn pop_front(&mut self) -> Option<T::Handle> {
        let head = self.head?;
        self.len -= 1;

        unsafe {
            let mut head_links = T::links(head);
            self.head = head_links.as_ref().next();
            if let Some(next) = head_links.as_mut().next() {
                T::links(next).as_mut().set_prev(None);
            } else {
                self.tail = None;
            }

            head_links.as_mut().unlink();
            Some(T::from_ptr(head))
        }
    }

    /// Removes an item from the tail of the list.
    pub fn pop_back(&mut self) -> Option<T::Handle> {
        let tail = self.tail?;
        self.len -= 1;

        unsafe {
            let mut tail_links = T::links(tail);
            // tracing::trace!(?self, tail.addr = ?tail, tail.links = ?tail_links, "pop_back");
            self.tail = tail_links.as_ref().prev();
            debug_assert_eq!(
                tail_links.as_ref().next(),
                None,
                "the tail node must not have a next link"
            );

            if let Some(prev) = tail_links.as_mut().prev() {
                T::links(prev).as_mut().set_next(None);
            } else {
                self.head = None;
            }

            tail_links.as_mut().unlink();
            // tracing::trace!(?self, tail.links = ?tail_links, "pop_back: popped");
            Some(T::from_ptr(tail))
        }
    }

    /// Remove an arbitrary node from the list.
    ///
    /// # Safety
    ///
    /// The caller *must* ensure that the removed node is an element of this
    /// linked list, and not any other linked list.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Handle> {
        let mut links = T::links(item);
        let links = links.as_mut();
        // tracing::trace!(?self, item.addr = ?item, item.links = ?links, "remove");
        let prev = links.set_prev(None);
        let next = links.set_next(None);

        if let Some(prev) = prev {
            T::links(prev).as_mut().set_next(next);
        } else if self.head != Some(item) {
            // tracing::trace!(?self.head, "item is not head, but has no prev; return None");
            return None;
        } else {
            debug_assert_ne!(Some(item), next, "node must not be linked to itself");
            self.head = next;
        }

        if let Some(next) = next {
            T::links(next).as_mut().set_prev(prev);
        } else if self.tail != Some(item) {
            // tracing::trace!(?self.tail, "item is not tail, but has no prev; return None");
            return None;
        } else {
            debug_assert_ne!(Some(item), prev, "node must not be linked to itself");
            self.tail = prev;
        }

        self.len -= 1;
        // tracing::trace!(?self, item.addr = ?item, "remove: done");
        Some(T::from_ptr(item))
    }

    /// Returns a [`Cursor`] over the items in this list.
    ///
    /// The [`Cursor`] type can be used as a mutable [`Iterator`]. In addition,
    /// however, it also permits modifying the *structure* of the list by
    /// inserting or removing elements at the cursor's current position.
    #[must_use]
    pub fn cursor(&mut self) -> Cursor<'_, T> {
        let len = self.len();
        Cursor {
            curr: self.head,
            list: self,
            len,
        }
    }

    /// Returns an iterator over the items in this list, by reference.
    #[must_use]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            _list: self,
            curr: self.head,
            curr_back: self.tail,
            len: self.len(),
        }
    }

    /// Returns an iterator over the items in this list, by mutable reference.
    #[must_use]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let curr = self.head;
        let curr_back = self.tail;
        let len = self.len();
        IterMut {
            _list: self,
            curr,
            curr_back,
            len,
        }
    }

    /// Returns an iterator which uses a closure to determine if an element
    /// should be removed from the list.
    ///
    /// If the closure returns `true`, then the element is removed and yielded.
    /// If the closure returns `false`, the element will remain in the list and
    /// will not be yielded by the iterator.
    ///
    /// Note that *unlike* the [`drain_filter` method][std-filter] on
    /// [`std::collections::LinkedList`], the closure is *not* permitted to
    /// mutate the elements of the list, as a mutable reference could be used to
    /// improperly unlink list nodes.
    ///
    /// [std-filter]: std::collections::LinkedList::drain_filter
    #[must_use]
    pub fn drain_filter<F>(&mut self, pred: F) -> DrainFilter<'_, T, F>
    where
        F: FnMut(&T) -> bool,
    {
        let curr = self.head;
        let old_len = self.len;
        DrainFilter {
            list: self,
            curr,
            pred,
            seen: 0,
            old_len,
        }
    }
}

unsafe impl<T: Linked<Links<T>> + ?Sized> Send for List<T> where T: Send {}
unsafe impl<T: Linked<Links<T>> + ?Sized> Sync for List<T> where T: Sync {}

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("List")
            .field("head", &FmtOption::new(&self.head))
            .field("tail", &FmtOption::new(&self.tail))
            .finish()
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> IntoIterator for &'list List<T> {
    type Item = &'list T;
    type IntoIter = Iter<'list, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> IntoIterator for &'list mut List<T> {
    type Item = Pin<&'list mut T>;
    type IntoIter = IterMut<'list, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T: Linked<Links<T>> + ?Sized> Drop for List<T> {
    fn drop(&mut self) {
        while let Some(node) = self.pop_front() {
            drop(node);
        }

        debug_assert!(self.is_empty());
    }
}

// ==== impl Links ====

impl<T: ?Sized> Links<T> {
    /// Returns new links for a [doubly-linked intrusive list](List).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(LinksInner {
                next: None,
                prev: None,
                _unpin: PhantomPinned,
            }),
        }
    }

    /// Returns `true` if this node is currently linked to a [`List`].
    pub fn is_linked(&self) -> bool {
        self.next().is_some() || self.prev().is_some()
    }

    fn unlink(&mut self) {
        self.inner.get_mut().next = None;
        self.inner.get_mut().prev = None;
    }

    #[inline]
    fn next(&self) -> Link<T> {
        unsafe { (*self.inner.get()).next }
    }

    #[inline]
    fn prev(&self) -> Link<T> {
        unsafe { (*self.inner.get()).prev }
    }

    #[inline]
    fn set_next(&mut self, next: Link<T>) -> Link<T> {
        mem::replace(&mut self.inner.get_mut().next, next)
    }

    #[inline]
    fn set_prev(&mut self, prev: Link<T>) -> Link<T> {
        mem::replace(&mut self.inner.get_mut().prev, prev)
    }

    fn assert_valid(&self, head: &Self, tail: &Self)
    where
        T: Linked<Self>,
    {
        if ptr::eq(self, head) {
            assert_eq!(
                self.prev(),
                None,
                "head node must not have a prev link; node={:#?}",
                self
            );
        }

        if ptr::eq(self, tail) {
            assert_eq!(
                self.next(),
                None,
                "tail node must not have a next link; node={:#?}",
                self
            );
        }

        assert_ne!(
            self.next(),
            self.prev(),
            "node cannot be linked in a loop; node={:#?}",
            self
        );

        if let Some(next) = self.next() {
            assert_ne!(
                unsafe { T::links(next) },
                NonNull::from(self),
                "node's next link cannot be to itself; node={:#?}",
                self
            );
        }
        if let Some(prev) = self.prev() {
            assert_ne!(
                unsafe { T::links(prev) },
                NonNull::from(self),
                "node's prev link cannot be to itself; node={:#?}",
                self
            );
        }
    }
}

impl<T: ?Sized> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ?Sized> fmt::Debug for Links<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Links")
            .field("self", &format_args!("{:p}", self))
            .field("next", &FmtOption::new(&self.next()))
            .field("prev", &FmtOption::new(&self.prev()))
            .finish()
    }
}

impl<T: ?Sized> PartialEq for Links<T> {
    fn eq(&self, other: &Self) -> bool {
        self.next() == other.next() && self.prev() == other.prev()
    }
}

/// # Safety
///
/// Types containing [`Links`] may be `Send`: the pointers within the `Links` may
/// mutably alias another value, but the links can only be _accessed_ by the
/// owner of the [`List`] itself, because the pointers are private. As long as
/// [`List`] upholds its own invariants, `Links` should not make a type `!Send`.
unsafe impl<T: Send> Send for Links<T> {}

/// # Safety
///
/// Types containing [`Links`] may be `Sync`: the pointers within the `Links` may
/// mutably alias another value, but the links can only be _accessed_ by the
/// owner of the [`List`] itself, because the pointers are private. As long as
/// [`List`] upholds its own invariants, `Links` should not make a type `!Sync`.
unsafe impl<T: Sync> Sync for Links<T> {}

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
                break;
            }
        }
        unsafe { self.list.remove(item?) }
    }
}

// === impl Iter ====

impl<'list, T: Linked<Links<T>> + ?Sized> Iterator for Iter<'list, T> {
    type Item = &'list T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }

        let curr = self.curr.take()?;
        self.len -= 1;
        unsafe {
            // safety: it is safe for us to borrow `curr`, because the iterator
            // borrows the `List`, ensuring that the list will not be dropped
            // while the iterator exists. the returned item will not outlive the
            // iterator.
            self.curr = T::links(curr).as_ref().next();
            Some(curr.as_ref())
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> ExactSizeIterator for Iter<'list, T> {
    #[inline]
    fn len(&self) -> usize {
        self.len
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> DoubleEndedIterator for Iter<'list, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }

        let curr = self.curr_back.take()?;
        self.len -= 1;
        unsafe {
            // safety: it is safe for us to borrow `curr`, because the iterator
            // borrows the `List`, ensuring that the list will not be dropped
            // while the iterator exists. the returned item will not outlive the
            // iterator.
            self.curr_back = T::links(curr).as_ref().prev();
            Some(curr.as_ref())
        }
    }
}

// === impl IterMut ====

impl<'list, T: Linked<Links<T>> + ?Sized> Iterator for IterMut<'list, T> {
    type Item = Pin<&'list mut T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }

        let mut curr = self.curr.take()?;
        self.len -= 1;
        unsafe {
            // safety: it is safe for us to borrow `curr`, because the iterator
            // borrows the `List`, ensuring that the list will not be dropped
            // while the iterator exists. the returned item will not outlive the
            // iterator.
            self.curr = T::links(curr).as_ref().next();

            // safety: pinning the returned element is actually *necessary* to
            // uphold safety invariants here. if we returned `&mut T`, the
            // element could be `mem::replace`d out of the list, invalidating
            // any pointers to it. thus, we *must* pin it before returning it.
            let pin = Pin::new_unchecked(curr.as_mut());
            Some(pin)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> ExactSizeIterator for IterMut<'list, T> {
    #[inline]
    fn len(&self) -> usize {
        self.len
    }
}

impl<'list, T: Linked<Links<T>> + ?Sized> DoubleEndedIterator for IterMut<'list, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.len == 0 {
            return None;
        }

        let mut curr = self.curr_back.take()?;
        self.len -= 1;
        unsafe {
            // safety: it is safe for us to borrow `curr`, because the iterator
            // borrows the `List`, ensuring that the list will not be dropped
            // while the iterator exists. the returned item will not outlive the
            // iterator.
            self.curr_back = T::links(curr).as_ref().prev();

            // safety: pinning the returned element is actually *necessary* to
            // uphold safety invariants here. if we returned `&mut T`, the
            // element could be `mem::replace`d out of the list, invalidating
            // any pointers to it. thus, we *must* pin it before returning it.
            let pin = Pin::new_unchecked(curr.as_mut());
            Some(pin)
        }
    }
}

// === impl DrainFilter ===

impl<T, F> Iterator for DrainFilter<'_, T, F>
where
    F: FnMut(&T) -> bool,
    T: Linked<Links<T>> + ?Sized,
{
    type Item = T::Handle;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(mut node) = self.curr {
            unsafe {
                self.curr = T::links(node).as_ref().next();
                self.seen += 1;

                if (self.pred)(node.as_mut()) {
                    return self.list.remove(node);
                }
            }
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.old_len - self.seen))
    }
}

impl<T, F> fmt::Debug for DrainFilter<'_, T, F>
where
    F: FnMut(&T) -> bool,
    T: Linked<Links<T>> + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrainFilter")
            .field("list", &self.list)
            .field("curr", &self.curr)
            .field("seen", &self.seen)
            .field("old_len", &self.old_len)
            .finish()
    }
}
