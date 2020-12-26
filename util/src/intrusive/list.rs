use core::fmt;
use core::mem::ManuallyDrop;
use core::ptr::NonNull;

pub unsafe trait Linked {
    type Handle;

    fn as_ptr(r: &Self::Handle) -> NonNull<Self>;
    unsafe fn as_handle(ptr: NonNull<Self>) -> Self::Handle;
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
    }

    pub fn pop_back(&mut self) -> Option<T::Handle> {
        let tail = self.tail?;
        unsafe {
            let mut tail_links = T::links(tail);
            self.tail = tail_links.as_ref().prev;

            if let Some(prev) = tail_links.as_mut().prev {
                T::links(prev).as_mut().next = None;
            } else {
                self.head = None;
            }
            tail_links.as_mut().unlink();
            Some(T::as_handle(tail))
        }
    }

    /// Remove a node from the list.
    ///
    /// # Safety
    ///
    /// The caller *must* ensure that the removed node is an element of this
    /// linked list, and not any other linked list.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Handle> {
        let Links { next, prev } = T::links(item).as_mut().take();

        if let Some(prev) = prev {
            T::links(prev).as_mut().next = next;
        } else if self.head != Some(item) {
            return None;
        } else {
            self.head = next;
        }

        if let Some(next) = next {
            T::links(next).as_mut().prev = prev;
        } else if self.tail != Some(item) {
            return None;
        } else {
            self.tail = prev;
        }

        Some(T::as_handle(item))
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
        self.next_ptr().map(|ptr| unsafe { T::as_handle(ptr) })
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
            Some(T::as_handle(curr))
        }
    }
}
