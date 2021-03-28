use core::{
    fmt,
    marker::PhantomPinned,
    mem::ManuallyDrop,
    ptr::{self, NonNull},
};

pub unsafe trait Linked {
    /// The handle owning nodes in the linked list.
    type Handle;

    /// Type of nodes in the linked list.
    ///
    /// When the type implementing `Linked` is not itself a reference, this is
    /// typically `Self`.
    ///
    /// # Safety
    ///
    /// This type may not be `Unpin`.
    type Node: ?Sized;

    /// Convert a `Handle` to a raw pointer, without consuming it.
    #[allow(clippy::wrong_self_convention)]
    fn as_ptr(r: &Self::Handle) -> NonNull<Self::Node>;

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self::Node>) -> Self::Handle;

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self::Node>) -> NonNull<Links<Self>>;
}

pub struct List<T: Linked + ?Sized> {
    head: Option<NonNull<T::Node>>,
    tail: Option<NonNull<T::Node>>,
}

pub struct Links<T: Linked + ?Sized> {
    next: Option<NonNull<T::Node>>,
    prev: Option<NonNull<T::Node>>,
    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// https://github.com/rust-lang/rust/issues/63818.
    _unpin: PhantomPinned,
}

pub struct Cursor<'a, T: Linked + ?Sized> {
    list: &'a mut List<T>,
    curr: Option<NonNull<T::Node>>,
}

pub struct Iter<'a, T: Linked + ?Sized> {
    _list: &'a List<T>,
    curr: Option<NonNull<T::Node>>,
}

// ==== impl List ====

impl<T: Linked + ?Sized> List<T> {
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
                "this should just never fucking happen lol"
            );
            assert_eq!(
                head_links.next, None,
                "if the linked list has only one node, it must not be linked"
            );
            assert_eq!(
                head_links.prev, None,
                "if the linked list has only one node, it must not be linked"
            );
            return;
        }

        let mut curr = Some(head);
        while let Some(node) = curr {
            let links = unsafe { T::links(node) };
            let links = unsafe { links.as_ref() };
            links.assert_valid(head_links, tail_links);
            curr = links.next;
        }
    }

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
    pub unsafe fn remove(&mut self, item: NonNull<T::Node>) -> Option<T::Handle> {
        let links = T::links(item).as_mut().take();
        tracing::trace!(?self, item.addr = ?item, item.links = ?links, "remove");
        let Links { next, prev, .. } = links;

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

unsafe impl<T: Linked + ?Sized> Send for List<T> where T::Node: Send {}
unsafe impl<T: Linked + ?Sized> Sync for List<T> where T::Node: Sync {}

impl<T: Linked + ?Sized> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("List")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .finish()
    }
}

// ==== impl Links ====

impl<T: Linked + ?Sized> Links<T> {
    pub const fn new() -> Self {
        Self {
            next: None,
            prev: None,
            _unpin: PhantomPinned,
        }
    }

    fn take(&mut self) -> Self {
        Self {
            next: self.next.take(),
            prev: self.next.take(),
            _unpin: PhantomPinned,
        }
    }

    fn unlink(&mut self) {
        self.take();
    }

    pub fn is_linked(&self) -> bool {
        self.next.is_some() || self.prev.is_some()
    }

    fn assert_valid(&self, head: &Self, tail: &Self)
    where
        T: Linked,
    {
        if ptr::eq(self, head) {
            assert_eq!(
                self.prev, None,
                "head node must not have a prev link; node={:#?}",
                self
            );
        }

        if ptr::eq(self, tail) {
            assert_eq!(
                self.next, None,
                "tail node must not have a next link; node={:#?}",
                self
            );
        }

        assert_ne!(
            self.next, self.prev,
            "node cannot be linked in a loop; node={:#?}",
            self
        );

        if let Some(next) = self.next {
            assert_ne!(
                unsafe { T::links(next) },
                NonNull::from(self),
                "node's next link cannot be to itself; node={:#?}",
                self
            );
        }
        if let Some(prev) = self.prev {
            assert_ne!(
                unsafe { T::links(prev) },
                NonNull::from(self),
                "node's prev link cannot be to itself; node={:#?}",
                self
            );
        }
    }
}

impl<T: Linked + ?Sized> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Linked + ?Sized> fmt::Debug for Links<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Links")
            .field("self", &format_args!("{:p}", self))
            .field("next", &self.next)
            .field("prev", &self.prev)
            .finish()
    }
}

impl<T: Linked + ?Sized> PartialEq for Links<T> {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next && self.prev == other.prev
    }
}

// === impl Cursor ====

impl<'a, T: Linked + ?Sized> Iterator for Cursor<'a, T> {
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_ptr().map(|ptr| unsafe { T::from_ptr(ptr) })
    }
}

impl<'a, T: Linked + ?Sized> Cursor<'a, T> {
    fn next_ptr(&mut self) -> Option<NonNull<T::Node>> {
        let curr = self.curr.take()?;
        self.curr = unsafe { T::links(curr).as_ref().next };
        Some(curr)
    }

    // Find and remove the first element matching a predicate.
    pub fn remove_first(
        &mut self,
        mut predicate: impl FnMut(&T::Node) -> bool,
    ) -> Option<T::Handle> {
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

impl<'a, T: Linked + ?Sized> Iterator for Iter<'a, T> {
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.curr.take()?;
        unsafe {
            self.curr = T::links(curr).as_ref().next;
            Some(T::from_ptr(curr))
        }
    }
}
