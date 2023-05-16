//! An intrusive doubly-linked list.
//!
//! See the [`List`] type for details.

use super::Linked;
use crate::util::FmtOption;
use core::{
    cell::UnsafeCell,
    fmt, iter,
    marker::PhantomPinned,
    mem,
    pin::Pin,
    ptr::{self, NonNull},
};

#[cfg(test)]
#[cfg(not(loom))]
mod tests;

mod cursor;
pub use self::cursor::{Cursor, CursorMut};

/// An [intrusive] doubly-linked list.
///
/// This data structure may be used as a first-in, first-out queue by using the
/// [`List::push_front`] and [`List::pop_back`] methods. It also supports
/// random-access removals using the [`List::remove`] method. This makes the
/// [`List`] type suitable for use in cases where elements must be able to drop
/// themselves while linked into a list.
///
/// This data structure can also be used as a stack or doubly-linked list by using
/// the [`List::pop_front`] and [`List::push_back`] methods.
///
/// The [`List`] type is **not** a lock-free data structure, and can only be
/// modified through `&mut` references.
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
/// use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
///
/// /// A simple queue entry that stores an `i32`.
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
///         // Using `ptr::addr_of_mut!` permits us to avoid creating a temporary
///         // reference without using layout-dependent casts.
///         let links = ptr::addr_of_mut!((*target.as_ptr()).links);
///
///         // `NonNull::new_unchecked` is safe to use here, because the pointer that
///         // we offset was not null, implying that the pointer produced by offsetting
///         // it will also not be null.
///         NonNull::new_unchecked(links)
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
/// # use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
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
/// #        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
/// #        NonNull::new_unchecked(links)
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
/// # use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
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
/// #        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
/// #        NonNull::new_unchecked(links)
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

/// An owning iterator over the elements of a [`List`].
///
/// This `struct` is created by the [`into_iter`] method on [`List`]
/// (provided by the [`IntoIterator`] trait). See its documentation for more.
///
/// [`into_iter`]: List::into_iter
/// [`IntoIterator`]: core::iter::IntoIterator
pub struct IntoIter<T: Linked<Links<T>> + ?Sized> {
    list: List<T>,
}

/// An iterator returned by [`List::drain_filter`].
pub struct DrainFilter<'list, T, F>
where
    F: FnMut(&T) -> bool,
    T: Linked<Links<T>> + ?Sized,
{
    cursor: CursorMut<'list, T>,
    pred: F,
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

    /// Moves all elements from `other` to the end of the list.
    ///
    /// This reuses all the nodes from `other` and moves them into `self`. After
    /// this operation, `other` becomes empty.
    ///
    /// This operation should complete in *O*(1) time and *O*(1) memory.
    pub fn append(&mut self, other: &mut Self) {
        // TODO(eliza): this could be rewritten to use `let ... else` when
        // that's supported on `cordyceps`' MSRV.
        let tail = match self.tail {
            Some(tail) => tail,
            None => {
                // if this list is empty, simply replace it with `other`
                debug_assert!(self.is_empty());
                mem::swap(self, other);
                return;
            }
        };

        // if `other` is empty, do nothing.
        if let Some((other_head, other_tail, other_len)) = other.take_all() {
            // attach the other list's head node to this list's tail node.
            unsafe {
                T::links(tail).as_mut().set_next(Some(other_head));
                T::links(other_head).as_mut().set_prev(Some(tail));
            }

            // this list's tail node is now the other list's tail node.
            self.tail = Some(other_tail);
            // this list's length increases by the other list's length, which
            // becomes 0.
            self.len += other_len;
        }
    }

    /// Attempts to split the list into two at the given index (inclusive).
    ///
    /// Returns everything after the given index (including the node at that
    /// index), or `None` if the index is greater than the list's [length].
    ///
    /// This operation should complete in *O*(*n*) time.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`List`]`<T>)` with a new list containing every element after
    ///   `at`, if `at` <= `self.len()`
    /// - [`None`] if `at > self.len()`
    ///
    /// [length]: Self::len
    pub fn try_split_off(&mut self, at: usize) -> Option<Self> {
        let len = self.len();
        // what is the index of the last node that should be left in this list?
        let split_idx = match at {
            // trying to split at the 0th index. we can just return the whole
            // list, leaving `self` empty.
            at if at == 0 => return Some(mem::replace(self, Self::new())),
            // trying to split at the last index. the new list will be empty.
            at if at == len => return Some(Self::new()),
            // we cannot split at an index that is greater than the length of
            // this list.
            at if at > len => return None,
            // otherwise, the last node in this list will be `at - 1`.
            at => at - 1,
        };

        let mut iter = self.iter();

        // advance to the node at `split_idx`, starting either from the head or
        // tail of the list.
        let dist_from_tail = len - 1 - split_idx;
        let split_node = if split_idx <= dist_from_tail {
            // advance from the head of the list.
            for _ in 0..split_idx {
                iter.next();
            }
            iter.curr
        } else {
            // advance from the tail of the list.
            for _ in 0..dist_from_tail {
                iter.next_back();
            }
            iter.curr_back
        };

        Some(unsafe { self.split_after_node(split_node, at) })
    }

    /// Split the list into two at the given index (inclusive).
    ///
    /// Every element after the given index, including the node at that
    /// index, is removed from this list, and returned as a new list.
    ///
    /// This operation should complete in *O*(*n*) time.
    ///
    /// # Returns
    ///
    /// A new [`List`]`<T>` containing every element after the index `at` in
    /// this list.
    ///
    /// # Panics
    ///
    /// If `at > self.len()`.
    #[track_caller]
    #[must_use]
    pub fn split_off(&mut self, at: usize) -> Self {
        match self.try_split_off(at) {
            Some(new_list) => new_list,
            None => panic!(
                "Cannot split off at a nonexistent index (the index was {} but the len was {})",
                at,
                self.len()
            ),
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
    #[track_caller]
    pub fn assert_valid(&self) {
        self.assert_valid_named("")
    }

    /// Asserts as many of the linked list's invariants as possible.
    #[track_caller]
    pub(crate) fn assert_valid_named(&self, name: &str) {
        // TODO(eliza): this could be rewritten to use `let ... else` when
        // that's supported on `cordyceps`' MSRV.
        let head = match self.head {
            Some(head) => head,
            None => {
                assert!(
                    self.tail.is_none(),
                    "{name}if the linked list's head is null, the tail must also be null"
                );
                assert_eq!(
                    self.len, 0,
                    "{name}if a linked list's head is null, its length must be 0"
                );
                return;
            }
        };

        assert_ne!(
            self.len, 0,
            "{name}if a linked list's head is not null, its length must be greater than 0"
        );

        assert_ne!(
            self.tail, None,
            "{name}if the linked list has a head, it must also have a tail"
        );
        let tail = self.tail.unwrap();

        let head_links = unsafe { T::links(head) };
        let tail_links = unsafe { T::links(tail) };
        let head_links = unsafe { head_links.as_ref() };
        let tail_links = unsafe { tail_links.as_ref() };
        if head == tail {
            assert_eq!(
                head_links, tail_links,
                "{name}if the head and tail nodes are the same, their links must be the same"
            );
            assert_eq!(
                head_links.next(),
                None,
                "{name}if the linked list has only one node, it must not be linked"
            );
            assert_eq!(
                head_links.prev(),
                None,
                "{name}if the linked list has only one node, it must not be linked"
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

        assert_eq!(
            self.len, actual_len,
            "{name}linked list's actual length did not match its `len` variable"
        );
    }

    /// Removes an item from the tail of the list.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// This returns a [`Handle`] that owns the popped element. Dropping the
    /// [`Handle`] will drop the element.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(T::Handle)` containing the last element of this list, if the
    ///   list was not empty.
    /// - [`None`] if this list is empty.
    ///
    /// [`Handle`]: crate::Linked::Handle
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

    /// Removes an item from the head of the list.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// This returns a [`Handle`] that owns the popped element. Dropping the
    /// [`Handle`] will drop the element.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(T::Handle)` containing the last element of this list, if the
    ///   list was not empty.
    /// - [`None`] if this list is empty.
    ///
    /// [`Handle`]: crate::Linked::Handle
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

    /// Appends an item to the tail of the list.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// This takes a [`Handle`] that owns the appended `item`. While the element
    /// is in the list, it is owned by the list, and will be dropped when the
    /// list is dropped. If the element is removed or otherwise unlinked from
    /// the list, ownership is assigned back to the [`Handle`].
    ///
    /// [`Handle`]: crate::Linked::Handle
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

    /// Appends an item to the head of the list.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// This takes a [`Handle`] that owns the appended `item`. While the element
    /// is in the list, it is owned by the list, and will be dropped when the
    /// list is dropped. If the element is removed or otherwise unlinked from
    /// the list, ownership is assigned back to the [`Handle`].
    ///
    /// [`Handle`]: crate::Linked::Handle
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

    /// Returns an immutable reference to the first element in the list.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// The node is [`Pin`]ned in memory, as moving it to a different memory
    /// location while it is in the list would corrupt the links pointing to
    /// that node.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`Pin`]`<&mut T>)` containing a pinned immutable reference to
    ///   the first element of the list, if the list is non-empty.
    /// - [`None`] if the list is empty.
    #[must_use]
    pub fn front(&self) -> Option<Pin<&T>> {
        let head = self.head?;
        let pin = unsafe {
            // NOTE(eliza): in this case, we don't *need* to pin the reference,
            // because it's immutable and you can't move out of a shared
            // reference in safe code. but...it makes the API more consistent
            // with `front_mut` etc.
            Pin::new_unchecked(head.as_ref())
        };
        Some(pin)
    }

    /// Returns a mutable reference to the first element in the list.
    ///
    /// The node is [`Pin`]ned in memory, as moving it to a different memory
    /// location while it is in the list would corrupt the links pointing to
    /// that node.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`Pin`]`<&mut T>)` containing a pinned mutable reference to
    ///   the first element of the list, if the list is non-empty.
    /// - [`None`] if the list is empty.
    #[must_use]
    pub fn front_mut(&mut self) -> Option<Pin<&mut T>> {
        let mut node = self.head?;
        let pin = unsafe {
            // safety: pinning the returned element is actually *necessary* to
            // uphold safety invariants here. if we returned `&mut T`, the
            // element could be `mem::replace`d out of the list, invalidating
            // any pointers to it. thus, we *must* pin it before returning it.
            Pin::new_unchecked(node.as_mut())
        };
        Some(pin)
    }

    /// Returns a reference to the last element in the list/
    ///
    /// The node is [`Pin`]ned in memory, as moving it to a different memory
    /// location while it is in the list would corrupt the links pointing to
    /// that node.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`Pin`]`<&T>)` containing a pinned immutable reference to
    ///   the last element of the list, if the list is non-empty.
    /// - [`None`] if the list is empty.
    #[must_use]
    pub fn back(&self) -> Option<Pin<&T>> {
        let node = self.tail?;
        let pin = unsafe {
            // NOTE(eliza): in this case, we don't *need* to pin the reference,
            // because it's immutable and you can't move out of a shared
            // reference in safe code. but...it makes the API more consistent
            // with `front_mut` etc.
            Pin::new_unchecked(node.as_ref())
        };
        Some(pin)
    }

    /// Returns a mutable reference to the last element in the list, or `None`
    /// if the list is empty.
    ///
    /// The node is [`Pin`]ned in memory, as moving it to a different memory
    /// location while it is in the list would corrupt the links pointing to
    /// that node.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(`[`Pin`]`<&T>)` containing a pinned mutable reference to
    ///   the last element of the list, if the list is non-empty.
    /// - [`None`] if the list is empty.
    #[must_use]
    pub fn back_mut(&mut self) -> Option<Pin<&mut T>> {
        let mut node = self.tail?;
        let pin = unsafe {
            // safety: pinning the returned element is actually *necessary* to
            // uphold safety invariants here. if we returned `&mut T`, the
            // element could be `mem::replace`d out of the list, invalidating
            // any pointers to it. thus, we *must* pin it before returning it.
            Pin::new_unchecked(node.as_mut())
        };
        Some(pin)
    }

    /// Remove an arbitrary node from the list.
    ///
    /// This operation should complete in *O*(1) time.
    ///
    /// This returns a [`Handle`] that owns the popped element. Dropping the
    /// [`Handle`] will drop the element.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(T::Handle)` containing a [`Handle`] that owns `item`, if
    ///   `item` is currently linked into this list.
    /// - [`None`] if `item` is not an element of this list.
    ///
    /// [`Handle`]: crate::Linked::Handle
    ///
    /// # Safety
    ///
    /// The caller *must* ensure that the removed node is an element of this
    /// linked list, and not any other linked list.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Handle> {
        let mut links = T::links(item);
        let links = links.as_mut();

        debug_assert!(
            !self.is_empty() || !links.is_linked(),
            "tried to remove an item from an empty list, but the item is linked!\n\
            is the item linked to a different list?\n  \
            item: {item:p}\n links: {links:?}\n  list: {self:?}\n"
        );

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

    /// Returns a [`CursorMut`] starting at the first element.
    ///
    /// The [`CursorMut`] type can be used as a mutable [`Iterator`]. In addition,
    /// however, it also permits modifying the *structure* of the list by
    /// inserting or removing elements at the cursor's current position.
    #[must_use]
    pub fn cursor_front_mut(&mut self) -> CursorMut<'_, T> {
        CursorMut::new(self, self.head, 0)
    }

    /// Returns a [`CursorMut`] starting at the last element.
    ///
    /// The [`CursorMut`] type can be used as a mutable [`Iterator`]. In addition,
    /// however, it also permits modifying the *structure* of the list by
    /// inserting or removing elements at the cursor's current position.
    #[must_use]
    pub fn cursor_back_mut(&mut self) -> CursorMut<'_, T> {
        let index = self.len().saturating_sub(1);
        CursorMut::new(self, self.tail, index)
    }

    /// Returns a [`Cursor`] starting at the first element.
    ///
    /// The [`Cursor`] type can be used as [`Iterator`] over this list. In
    /// addition, it may be seeked back and forth to an arbitrary position in
    /// the list.
    #[must_use]
    pub fn cursor_front(&self) -> Cursor<'_, T> {
        Cursor::new(self, self.head, 0)
    }

    /// Returns a [`Cursor`] starting at the last element.
    ///
    /// The [`Cursor`] type can be used as [`Iterator`] over this list. In
    /// addition, it may be seeked back and forth to an arbitrary position in
    /// the list.
    #[must_use]
    pub fn cursor_back(&self) -> Cursor<'_, T> {
        let index = self.len().saturating_sub(1);
        Cursor::new(self, self.tail, index)
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
        let cursor = self.cursor_front_mut();
        DrainFilter { cursor, pred }
    }

    /// Inserts the list segment represented by `splice_start` and `splice_end`
    /// between `next` and `prev`.
    ///
    /// # Safety
    ///
    /// This method requires the following invariants be upheld:
    ///
    /// - `prev` and `next` are part of the same list.
    /// - `prev` and `next` are not the same node.
    /// - `splice_start` and `splice_end` are part of the same list, which is
    ///   *not* the same list that `prev` and `next` are part of.
    /// -`prev` is `next`'s `prev` node, and `next` is `prev`'s `prev` node.
    /// - `splice_start` is ahead of `splice_end` in the list that they came from.
    #[inline]
    unsafe fn insert_nodes_between(
        &mut self,
        prev: Link<T>,
        next: Link<T>,
        splice_start: NonNull<T>,
        splice_end: NonNull<T>,
        spliced_length: usize,
    ) {
        debug_assert!(
            (prev.is_none() && next.is_none()) || prev != next,
            "cannot insert between a node and itself!\n    \
            prev: {prev:?}\n   next: {next:?}",
        );
        // This method takes care not to create multiple mutable references to
        // whole nodes at the same time, to maintain validity of aliasing
        // pointers into `element`.

        if let Some(prev) = prev {
            let links = T::links(prev).as_mut();
            debug_assert_eq!(links.next(), next);
            links.set_next(Some(splice_start));
        } else {
            self.head = Some(splice_start);
        }

        if let Some(next) = next {
            let links = T::links(next).as_mut();
            debug_assert_eq!(links.prev(), prev);
            links.set_prev(Some(splice_end));
        } else {
            self.tail = Some(splice_end);
        }

        let start_links = T::links(splice_start).as_mut();
        let end_links = T::links(splice_end).as_mut();
        debug_assert!(
            splice_start == splice_end
                || (start_links.next().is_some() && end_links.prev().is_some()),
            "splice_start must be ahead of splice_end!\n   \
            splice_start: {splice_start:?}\n    \
            splice_end: {splice_end:?}\n  \
            start_links: {start_links:?}\n   \
            end_links: {end_links:?}",
        );

        start_links.set_prev(prev);
        end_links.set_next(next);

        self.len += spliced_length;
    }

    #[inline]
    unsafe fn split_after_node(&mut self, split_node: Link<T>, idx: usize) -> Self {
        // TODO(eliza): this could be rewritten to use `let ... else` when
        // that's supported on `cordyceps`' MSRV.
        let split_node = match split_node {
            Some(node) => node,
            None => return mem::replace(self, Self::new()),
        };

        // the head of the new list is the split node's `next` node (which is
        // replaced with `None`)
        let head = unsafe { T::links(split_node).as_mut().set_next(None) };
        let tail = if let Some(head) = head {
            // since `head` is now the head of its own list, it has no `prev`
            // link any more.
            let _prev = unsafe { T::links(head).as_mut().set_prev(None) };
            debug_assert_eq!(_prev, Some(split_node));

            // the tail of the new list is this list's old tail, if the split list
            // is not empty.
            self.tail.replace(split_node)
        } else {
            None
        };

        let split = Self {
            head,
            tail,
            len: self.len - idx,
        };

        // update this list's length (note that this occurs after constructing
        // the new list, because we use this list's length to determine the new
        // list's length).
        self.len = idx;

        split
    }

    /// Empties this list, returning its head, tail, and length if it is
    /// non-empty. If the list is empty, this returns `None`.
    #[inline]
    fn take_all(&mut self) -> Option<(NonNull<T>, NonNull<T>, usize)> {
        let head = self.head.take()?;
        let tail = self.tail.take();
        debug_assert!(
            tail.is_some(),
            "if a list's `head` is `Some`, its tail must also be `Some`"
        );
        let tail = tail?;
        let len = mem::replace(&mut self.len, 0);
        debug_assert_ne!(
            len, 0,
            "if a list is non-empty, its `len` must be greater than 0"
        );
        Some((head, tail, len))
    }
}

impl<T> iter::Extend<T::Handle> for List<T>
where
    T: Linked<Links<T>> + ?Sized,
{
    fn extend<I: IntoIterator<Item = T::Handle>>(&mut self, iter: I) {
        for item in iter {
            self.push_back(item);
        }
    }

    // TODO(eliza): when `Extend::extend_one` becomes stable, implement that
    // as well, so that we can just call `push_back` without looping.
}

impl<T> iter::FromIterator<T::Handle> for List<T>
where
    T: Linked<Links<T>> + ?Sized,
{
    fn from_iter<I: IntoIterator<Item = T::Handle>>(iter: I) -> Self {
        let mut list = Self::new();
        list.extend(iter);
        list
    }
}

unsafe impl<T: Linked<Links<T>> + ?Sized> Send for List<T> where T: Send {}
unsafe impl<T: Linked<Links<T>> + ?Sized> Sync for List<T> where T: Sync {}

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { head, tail, len } = self;
        f.debug_struct("List")
            .field("head", &FmtOption::new(head))
            .field("tail", &FmtOption::new(tail))
            .field("len", len)
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

impl<T: Linked<Links<T>> + ?Sized> IntoIterator for List<T> {
    type Item = T::Handle;
    type IntoIter = IntoIter<T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        IntoIter { list: self }
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
                "head node must not have a prev link; node={self:#?}",
            );
        }

        if ptr::eq(self, tail) {
            assert_eq!(
                self.next(),
                None,
                "tail node must not have a next link; node={self:#?}",
            );
        }

        assert_ne!(
            self.next(),
            self.prev(),
            "node cannot be linked in a loop; node={self:#?}",
        );

        if let Some(next) = self.next() {
            assert_ne!(
                unsafe { T::links(next) },
                NonNull::from(self),
                "node's next link cannot be to itself; node={self:#?}",
            );
        }
        if let Some(prev) = self.prev() {
            assert_ne!(
                unsafe { T::links(prev) },
                NonNull::from(self),
                "node's prev link cannot be to itself; node={self:#?}",
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
            .field("self", &format_args!("{self:p}"))
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

impl<'list, T: Linked<Links<T>> + ?Sized> iter::FusedIterator for Iter<'list, T> {}

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

impl<'list, T: Linked<Links<T>> + ?Sized> iter::FusedIterator for IterMut<'list, T> {}

// === impl IntoIter ===

impl<T: Linked<Links<T>> + ?Sized> fmt::Debug for IntoIter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { list } = self;
        f.debug_tuple("IntoIter").field(list).finish()
    }
}

impl<T: Linked<Links<T>> + ?Sized> Iterator for IntoIter<T> {
    type Item = T::Handle;

    #[inline]
    fn next(&mut self) -> Option<T::Handle> {
        self.list.pop_front()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.list.len, Some(self.list.len))
    }
}

impl<T: Linked<Links<T>> + ?Sized> DoubleEndedIterator for IntoIter<T> {
    #[inline]
    fn next_back(&mut self) -> Option<T::Handle> {
        self.list.pop_back()
    }
}

impl<T: Linked<Links<T>> + ?Sized> ExactSizeIterator for IntoIter<T> {
    #[inline]
    fn len(&self) -> usize {
        self.list.len
    }
}

impl<T: Linked<Links<T>> + ?Sized> iter::FusedIterator for IntoIter<T> {}

// === impl DrainFilter ===

impl<T, F> Iterator for DrainFilter<'_, T, F>
where
    F: FnMut(&T) -> bool,
    T: Linked<Links<T>> + ?Sized,
{
    type Item = T::Handle;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.cursor.remove_first(&mut self.pred)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.cursor.len()))
    }
}

impl<T, F> fmt::Debug for DrainFilter<'_, T, F>
where
    F: FnMut(&T) -> bool,
    T: Linked<Links<T>> + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { cursor, pred: _ } = self;
        f.debug_struct("DrainFilter")
            .field("cursor", cursor)
            .field("pred", &format_args!("..."))
            .finish()
    }
}
