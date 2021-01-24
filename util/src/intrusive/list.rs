use core::fmt;
use core::mem::ManuallyDrop;
use core::ptr::NonNull;

pub unsafe trait Linked {
    type Handle;

    /// Convert a `Handle` to a raw pointer, without consuming it.
    #[allow(clippy::wrong_self_convention)]
    fn as_ptr(r: &Self::Handle) -> NonNull<Self>;

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle;

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<Links<Self>>;
}

pub struct List<T: ?Sized> {
    head: Option<NonNull<T>>,
    tail: Option<NonNull<T>>,
}

pub struct Links<T: ?Sized> {
    next: Option<NonNull<T>>,
    prev: Option<NonNull<T>>,
}

pub struct Cursor<'a, T: ?Sized + Linked> {
    list: &'a mut List<T>,
    curr: Option<NonNull<T>>,
}

pub struct Iter<'a, T: ?Sized + Linked> {
    _list: &'a List<T>,
    curr: Option<NonNull<T>>,
}

// ==== impl List ====

impl<T: ?Sized> List<T> {
    /// Returns a new empty list.
    pub const fn new() -> List<T> {
        List {
            head: None,
            tail: None,
        }
    }

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

impl<T: ?Sized + Linked> List<T> {
    /// Appends an item to the head of the list.
    pub fn push_front(&mut self, item: T::Handle) {
        let item = ManuallyDrop::new(item);
        let ptr = T::as_ptr(&*item);
        tracing::trace!(?self, ?ptr, "push_front");
        assert_ne!(self.head, Some(ptr));
        unsafe {
            let mut links = T::links(ptr);
            links.as_mut().next = self.head;
            links.as_mut().prev = None;

            if let Some(head) = self.head {
                T::links(head).as_mut().prev = Some(ptr);
            }
        }

        self.head = Some(ptr);

        if self.tail.is_none() {
            self.tail = Some(ptr);
        }

        tracing::trace!(?self, "push_front: pushed");
    }

    pub fn pop_back(&mut self) -> Option<T::Handle> {
        let tail = self.tail?;
        unsafe {
            let mut tail_links = T::links(tail);
            tracing::trace!(?self, tail.addr = ?tail, tail.links = ?tail_links, "pop_back");
            self.tail = tail_links.as_ref().prev;
            debug_assert_eq!(
                tail_links.as_ref().next,
                None,
                "the tail node must not have a next link"
            );

            if let Some(prev) = tail_links.as_mut().prev {
                debug_assert_ne!(
                    self.head,
                    Some(tail),
                    "a node with a previous link should not be the list head"
                );
                T::links(prev).as_mut().next = None;
            } else {
                debug_assert_eq!(
                    self.head,
                    Some(tail),
                    "if the tail has no previous link, it must be the head"
                );
                self.head = None;
            }
            tail_links.as_mut().unlink();
            tracing::trace!(?self, tail.links = ?tail_links, "pop_back: popped");
            Some(T::from_ptr(tail))
        }
    }

    /// Remove a node from the list.
    ///
    /// # Safety
    ///
    /// The caller *must* ensure that the removed node is an element of this
    /// linked list, and not any other linked list.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Handle> {
        let links = T::links(item).as_mut().take();
        tracing::trace!(?self, item.addr = ?item, item.links = ?links, "remove");
        let Links { next, prev } = links;

        if let Some(prev) = prev {
            T::links(prev).as_mut().next = next;
        } else if self.head != Some(item) {
            return None;
        } else {
            debug_assert_ne!(Some(item), next, "node must not be linked to itself");
            self.head = next;
        }

        if let Some(next) = next {
            T::links(next).as_mut().prev = prev;
        } else if self.tail != Some(item) {
            return None;
        } else {
            debug_assert_ne!(Some(item), prev, "node must not be linked to itself");
            self.tail = prev;
        }

        tracing::trace!(?self, item.addr = ?item, "remove: done");
        Some(T::from_ptr(item))
    }

    pub fn cursor(&mut self) -> Cursor<'_, T> {
        Cursor {
            curr: self.head,
            list: self,
        }
    }
}

unsafe impl<T: Linked> Send for List<T> where T: Send {}
unsafe impl<T: Linked> Sync for List<T> where T: Sync {}

impl<T: ?Sized> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("List")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .finish()
    }
}

// ==== impl Links ====

impl<T: ?Sized> Links<T> {
    pub const fn new() -> Self {
        Self {
            next: None,
            prev: None,
        }
    }

    fn take(&mut self) -> Self {
        Self {
            next: self.next.take(),
            prev: self.next.take(),
        }
    }

    fn unlink(&mut self) {
        self.take();
    }

    pub fn is_linked(&self) -> bool {
        self.next.is_some() || self.prev.is_some()
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
            .field("next", &self.next)
            .field("prev", &self.prev)
            .finish()
    }
}

// === impl Cursor ====

impl<'a, T: ?Sized + Linked> Iterator for Cursor<'a, T> {
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_ptr().map(|ptr| unsafe { T::from_ptr(ptr) })
    }
}

impl<'a, T: ?Sized + Linked> Cursor<'a, T> {
    fn next_ptr(&mut self) -> Option<NonNull<T>> {
        let curr = self.curr.take()?;
        self.curr = unsafe { T::links(curr).as_ref().next };
        Some(curr)
    }

    // Find and remove the first element matching a predicate.
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

impl<'a, T: ?Sized + Linked> Iterator for Iter<'a, T> {
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.curr.take()?;
        unsafe {
            self.curr = T::links(curr).as_ref().next;
            Some(T::from_ptr(curr))
        }
    }
}
