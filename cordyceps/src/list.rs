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
    ptr::{self, NonNull},
};

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
pub struct List<T: ?Sized> {
    head: Link<T>,
    tail: Link<T>,
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
pub struct Cursor<'a, T: Linked<Links<T>> + ?Sized> {
    list: &'a mut List<T>,
    curr: Link<T>,
}

/// Iterates over the items in a [`List`] by reference.
pub struct Iter<'a, T: Linked<Links<T>> + ?Sized> {
    _list: &'a List<T>,
    curr: Link<T>,
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
impl<T: ?Sized> List<T> {
    /// Returns a new empty list.
    #[must_use]
    pub const fn new() -> List<T> {
        List {
            head: None,
            tail: None,
        }
    }

    /// Returns `true` if this list is empty.
    pub fn is_empty(&self) -> bool {
        if self.head.is_none() {
            debug_assert!(
                self.tail.is_none(),
                "inconsistent state: a list had a tail but no head!"
            );
            return true;
        }

        false
    }
}

impl<T: Linked<Links<T>> + ?Sized> List<T> {
    /// Asserts as many of the linked list's invariants as possible.
    pub fn assert_valid(&self) {
        let head = match self.head {
            Some(head) => head,
            None => {
                assert!(
                    self.tail.is_none(),
                    "if the linked list's head is null, the tail must also be null"
                );
                return;
            }
        };

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
        while let Some(node) = curr {
            let links = unsafe { T::links(node) };
            let links = unsafe { links.as_ref() };
            links.assert_valid(head_links, tail_links);
            curr = links.next();
        }
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
    }

    /// Remove an item from the head of the list
    pub fn pop_front(&mut self) -> Option<T::Handle> {
        let head = self.head?;

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
        Cursor {
            curr: self.head,
            list: self,
        }
    }

    /// Returns an iterator over the items in this list, by reference.
    #[must_use]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            _list: self,
            curr: self.head,
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
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_ptr().map(|ptr| unsafe { T::from_ptr(ptr) })
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

// TODO(eliza): next_back

// === impl Iter ====

impl<'a, T: Linked<Links<T>> + ?Sized> Iterator for Iter<'a, T> {
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.curr.take()?;
        unsafe {
            self.curr = T::links(curr).as_ref().next();
            Some(T::from_ptr(curr))
        }
    }
}

#[cfg(test)]
#[cfg(not(loom))]
mod tests {
    use super::*;

    use std::{boxed::Box, pin::Pin, vec, vec::Vec};

    #[derive(Debug)]
    #[repr(C)]
    struct Entry<'a> {
        links: Links<Entry<'a>>,
        val: i32,
        _lt: std::marker::PhantomData<&'a ()>,
    }

    unsafe impl<'a> Linked<Links<Self>> for Entry<'a> {
        type Handle = Pin<&'a Entry<'a>>;

        fn into_ptr(handle: Pin<&'a Entry<'a>>) -> NonNull<Entry<'a>> {
            NonNull::from(handle.get_ref())
        }

        unsafe fn from_ptr(ptr: NonNull<Entry<'a>>) -> Pin<&'a Entry<'a>> {
            // Safety: if this function is only called by the linked list
            // implementation (and it is not intended for external use), we can
            // expect that the `NonNull` was constructed from a reference which
            // was pinned.
            //
            // If other callers besides `List`'s internals were to call this on
            // some random `NonNull<Entry>`, this would not be the case, and
            // this could be constructing an erroneous `Pin` from a referent
            // that may not be pinned!
            Pin::new_unchecked(&*ptr.as_ptr())
        }

        unsafe fn links(target: NonNull<Entry<'a>>) -> NonNull<Links<Entry<'a>>> {
            // Safety: this is safe because the `links` are the first field of
            // `Entry`, and `Entry` is `repr(C)`.
            target.cast()
        }
    }

    fn entry<'a>(val: i32) -> Pin<Box<Entry<'a>>> {
        Box::pin(Entry {
            links: Links::new(),
            val,
            _lt: std::marker::PhantomData,
        })
    }

    fn ptr<'a>(r: &Pin<Box<Entry<'a>>>) -> NonNull<Entry<'a>> {
        r.as_ref().get_ref().into()
    }

    fn collect_list(list: &mut List<Entry<'_>>) -> Vec<i32> {
        let mut ret = vec![];

        while let Some(entry) = list.pop_back() {
            ret.push(entry.val);
        }

        ret
    }

    fn push_all<'a>(list: &mut List<Entry<'a>>, entries: &[Pin<&'a Entry<'a>>]) {
        for entry in entries.iter() {
            list.push_front(*entry);
        }
    }

    macro_rules! assert_clean {
        ($e:ident) => {{
            assert!(!$e.links.is_linked())
        }};
    }

    macro_rules! assert_ptr_eq {
        ($a:expr, $b:expr) => {{
            // Deal with mapping a Pin<&mut T> -> Link<T>
            assert_eq!(Some($a.as_ref().get_ref().into()), $b)
        }};
    }

    #[test]
    fn const_new() {
        const _: List<Entry> = List::new();
    }

    fn trace_init() -> tracing::dispatcher::DefaultGuard {
        use tracing_subscriber::prelude::*;
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::TRACE)
            .with_target(false)
            .with_timer(())
            .set_default()
    }

    #[test]
    fn push_and_drain() {
        let _trace = trace_init();

        let a = entry(5);
        let b = entry(7);
        let c = entry(31);

        let mut list = List::new();
        assert!(list.is_empty());

        list.push_front(a.as_ref());
        assert!(!list.is_empty());
        list.assert_valid();
        list.push_front(b.as_ref());
        list.assert_valid();
        list.push_front(c.as_ref());
        list.assert_valid();

        let items: Vec<i32> = collect_list(&mut list);
        assert_eq!([5, 7, 31].to_vec(), items);

        list.assert_valid();
        assert!(list.is_empty());
    }

    #[test]
    fn pop_front() {
        let _trace = trace_init();

        let a = entry(5);
        let b = entry(7);
        let c = entry(9);
        let mut list = List::<Entry>::new();

        list.push_front(a.as_ref());
        list.assert_valid();

        list.push_front(b.as_ref());
        list.assert_valid();

        list.push_front(c.as_ref());
        list.assert_valid();

        let d = list.pop_front().unwrap();
        assert_eq!(9, d.val);

        let e = list.pop_front().unwrap();
        assert_eq!(7, e.val);

        let f = list.pop_front().unwrap();
        assert_eq!(5, f.val);

        assert!(list.is_empty());
        assert!(list.pop_front().is_none());
        list.assert_valid();
    }

    #[test]
    fn push_back() {
        let _trace = trace_init();

        let a = entry(5);
        let b = entry(7);
        let c = entry(9);
        let mut list = List::<Entry>::new();

        list.push_back(a.as_ref());
        list.assert_valid();

        list.push_back(b.as_ref());
        list.assert_valid();

        list.push_back(c.as_ref());
        list.assert_valid();

        let d = list.pop_back().unwrap();
        assert_eq!(9, d.val);

        let e = list.pop_back().unwrap();
        assert_eq!(7, e.val);

        let f = list.pop_back().unwrap();
        assert_eq!(5, f.val);

        assert!(list.is_empty());
        assert!(list.pop_back().is_none());

        list.assert_valid();
    }

    #[test]
    fn push_pop_push_pop() {
        let _trace = trace_init();

        let a = entry(5);
        let b = entry(7);

        let mut list = List::<Entry>::new();

        list.push_front(a.as_ref());
        list.assert_valid();

        let entry = list.pop_back().unwrap();
        assert_eq!(5, entry.val);
        assert!(list.is_empty());
        list.assert_valid();

        list.push_front(b.as_ref());
        list.assert_valid();

        let entry = list.pop_back().unwrap();
        assert_eq!(7, entry.val);
        list.assert_valid();

        assert!(list.is_empty());
        assert!(list.pop_back().is_none());
        list.assert_valid();
    }

    mod remove_by_address {
        use super::*;

        #[test]
        fn first() {
            let _trace = trace_init();
            let a = entry(5);
            let b = entry(7);
            let c = entry(31);

            unsafe {
                // Remove first
                let mut list = List::new();

                push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);
                assert!(list.remove(ptr(&a)).is_some());
                assert_clean!(a);
                list.assert_valid();

                // `a` should be no longer there and can't be removed twice
                assert!(list.remove(ptr(&a)).is_none());
                assert!(!list.is_empty());
                list.assert_valid();

                assert!(list.remove(ptr(&b)).is_some());
                assert_clean!(b);
                list.assert_valid();

                // `b` should be no longer there and can't be removed twice
                assert!(list.remove(ptr(&b)).is_none());
                assert!(!list.is_empty());
                list.assert_valid();

                assert!(list.remove(ptr(&c)).is_some());
                assert_clean!(c);
                list.assert_valid();
                // `b` should be no longer there and can't be removed twice
                assert!(list.remove(ptr(&c)).is_none());
                assert!(list.is_empty());
                list.assert_valid();
            }

            unsafe {
                // Remove first of two
                let mut list = List::new();

                push_all(&mut list, &[b.as_ref(), a.as_ref()]);

                assert!(list.remove(ptr(&a)).is_some());
                assert_clean!(a);
                list.assert_valid();

                // a should be no longer there and can't be removed twice
                assert!(list.remove(ptr(&a)).is_none());
                list.assert_valid();

                assert_ptr_eq!(b, list.head);
                assert_ptr_eq!(b, list.tail);

                assert!(b.links.next().is_none());
                assert!(b.links.prev().is_none());

                let items = collect_list(&mut list);
                assert_eq!([7].to_vec(), items);
            }
        }

        #[test]
        fn middle() {
            let _trace = trace_init();

            let a = entry(5);
            let b = entry(7);
            let c = entry(31);

            unsafe {
                let mut list = List::new();

                push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);

                assert!(list.remove(ptr(&a)).is_some());
                assert_clean!(a);
                list.assert_valid();

                assert_ptr_eq!(b, list.head);
                assert_ptr_eq!(c, b.links.next());
                assert_ptr_eq!(b, c.links.prev());

                let items = collect_list(&mut list);
                assert_eq!([31, 7].to_vec(), items);
                list.assert_valid();
            }

            unsafe {
                let mut list = List::new();

                push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);

                assert!(list.remove(ptr(&b)).is_some());
                assert_clean!(b);
                list.assert_valid();

                assert_ptr_eq!(c, a.links.next());
                assert_ptr_eq!(a, c.links.prev());

                let items = collect_list(&mut list);
                assert_eq!([31, 5].to_vec(), items);
            }
        }

        #[test]
        fn last_middle() {
            let _trace = trace_init();

            let a = entry(5);
            let b = entry(7);
            let c = entry(31);

            unsafe {
                // Remove last
                // Remove middle
                let mut list = List::new();

                push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);

                assert!(list.remove(ptr(&c)).is_some());
                assert_clean!(c);
                list.assert_valid();

                assert!(b.links.next().is_none());
                assert_ptr_eq!(b, list.tail);

                let items = collect_list(&mut list);
                assert_eq!([7, 5].to_vec(), items);
            }
        }

        #[test]
        fn last() {
            let _trace = trace_init();

            let a = entry(5);
            let b = entry(7);

            unsafe {
                // Remove last item
                let mut list = List::new();

                push_all(&mut list, &[a.as_ref()]);

                assert!(list.remove(ptr(&a)).is_some());
                assert_clean!(a);
                list.assert_valid();

                assert!(list.head.is_none());
                assert!(list.tail.is_none());
                let items = collect_list(&mut list);
                assert!(items.is_empty());
            }

            unsafe {
                // Remove last of two
                let mut list = List::new();

                push_all(&mut list, &[b.as_ref(), a.as_ref()]);

                assert!(list.remove(ptr(&b)).is_some());
                assert_clean!(b);
                list.assert_valid();

                assert_ptr_eq!(a, list.head);
                assert_ptr_eq!(a, list.tail);

                assert!(a.links.next().is_none());
                assert!(a.links.prev().is_none());

                let items = collect_list(&mut list);
                assert_eq!([5].to_vec(), items);
            }
        }

        #[test]
        fn missing() {
            let _trace = trace_init();

            let a = entry(5);
            let b = entry(7);
            let c = entry(31);
            unsafe {
                // Remove missing
                let mut list = List::<Entry<'_>>::new();

                list.push_front(b.as_ref());
                list.push_front(a.as_ref());

                assert!(list.remove(ptr(&c)).is_none());
                list.assert_valid();
            }
        }
    }

    // #[test]
    // fn cursor() {
    //     let _trace = trace_init();

    //     let a = entry(5);
    //     let b = entry(7);

    //     let mut list = List::<Entry<'_>>::new();

    //     assert_eq!(0, list.cursor().count());

    //     list.push_front(a.as_ref());
    //     list.push_front(b.as_ref());

    //     let mut i = list.cursor();
    //     assert_eq!(7, i.next().unwrap().val);
    //     assert_eq!(5, i.next().unwrap().val);
    //     assert!(i.next().is_none());
    // }

    #[derive(Debug)]
    enum Op {
        PushFront,
        PopBack,
        PushBack,
        PopFront,
        Remove(usize),
    }

    use core::ops::Range;
    use proptest::collection::vec;
    use proptest::num::usize::ANY;

    /// Miri uses a significant amount of time and memory, meaning that
    /// running 256 property tests (the default test-pass count) * (0..100)
    /// vec elements (the default proptest vec length strategy) causes the
    /// CI running to OOM (I think). In local testing, this required up
    /// to 11GiB of resident memory with the default strategy, at the
    /// time of this change.
    ///
    /// In the future, it may be desirable to have an "override" feature
    /// to use a larger test case set for more exhaustive local miri testing,
    /// where the time and memory limitations are less restrictive than in CI.
    #[cfg(miri)]
    const FUZZ_RANGE: Range<usize> = 0..10;

    /// The default range for proptest's vec strategy is 0..100.
    #[cfg(not(miri))]
    const FUZZ_RANGE: Range<usize> = 0..100;

    proptest::proptest! {
        #[test]
        fn fuzz_linked_list(ops in vec(ANY, FUZZ_RANGE)) {

            let ops = ops
                .iter()
                .map(|i| match i % 5 {
                    0 => Op::PushFront,
                    1 => Op::PopBack,
                    2 => Op::PushBack,
                    3 => Op::PopFront,
                    4 => Op::Remove(i / 5),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();

            let _trace = trace_init();
            let _span = tracing::info_span!("fuzz").entered();
            tracing::info!(?ops);
            run_fuzz(ops);
        }
    }

    fn run_fuzz(ops: Vec<Op>) {
        use std::collections::VecDeque;

        let mut ll = List::<Entry<'_>>::new();
        let mut reference = VecDeque::new();

        let entries: Vec<_> = (0..ops.len()).map(|i| entry(i as i32)).collect();

        for (i, op) in ops.iter().enumerate() {
            let _span = tracing::info_span!("op", ?i, ?op).entered();
            tracing::info!(?op);
            match op {
                Op::PushFront => {
                    reference.push_front(i as i32);
                    assert_eq!(entries[i].val, i as i32);

                    ll.push_front(entries[i].as_ref());
                }
                Op::PopBack => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        tracing::debug!("skipping pop; list is empty");
                        continue;
                    }

                    let v = reference.pop_back();
                    assert_eq!(v, ll.pop_back().map(|v| v.val));
                }
                Op::PushBack => {
                    reference.push_back(i as i32);
                    assert_eq!(entries[i].val, i as i32);

                    ll.push_back(entries[i].as_ref());
                }
                Op::PopFront => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        tracing::debug!("skipping pop: list is empty");
                        continue;
                    }

                    let v = reference.pop_front();
                    assert_eq!(v, ll.pop_front().map(|v| v.val));
                }
                Op::Remove(n) => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());

                        tracing::debug!("skipping re; list is empty");
                        continue;
                    }

                    let idx = n % reference.len();
                    let expect = reference.remove(idx).unwrap();

                    unsafe {
                        let entry = ll.remove(ptr(&entries[expect as usize])).unwrap();
                        assert_eq!(expect, entry.val);
                    }
                }
            }
            ll.assert_valid();
        }
    }
}
