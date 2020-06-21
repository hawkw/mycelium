use core::fmt;
use core::mem::ManuallyDrop;
use core::ptr::NonNull;

pub unsafe trait Linked {
    type Ref;

    fn as_ptr(r: &Self::Ref) -> NonNull<Self>;
    unsafe fn from_ptr(self: NonNull<Self>) -> Self::Ref;
    unsafe fn links(self: NonNull<Self>) -> NonNull<Links<Self>>;
}

pub struct List<T: ?Sized + Linked> {
    head: Option<NonNull<T>>,
    tail: Option<NonNull<T>>,
}

pub struct Links<T: ?Sized> {
    next: Option<NonNull<T>>,
    prev: Option<NonNull<T>>,
}

// ==== impl List ====

impl<T: ?Sized + Linked> List<T> {
    /// Returns a new empty list.
    pub const fn new() -> List<T> {
        List {
            head: None,
            tail: None,
        }
    }

    pub fn is_empty() -> bool {
        if self.head.is_none() {
            debug_assert!(
                self.tail.is_none(),
                "inconsistent state: a list had a tail but no head!"
            );
            return true;
        }

        false
    }

    /// Appends an item to the head of the list.
    pub fn push_front(&mut self, item: T::Ref) {
        let item = ManuallyDrop::new(item);
        let ptr = T::as_ptr(&*item);
        assert_ne!(self.head, Some(ptr));
        unsafe {
            let links = t.links().as_mut();
            links.next = self.head;
            links.prev = None;

            if let Some(head) = self.head {
                head.links().as_mut().prev = Some(ptr);
            }
        }

        self.head = Some(ptr);

        if self.tail.is_none() {
            self.tail = Some(ptr);
        }
    }

    pub fn pop_back(&mut self) -> Option<T::Ref> {
        let tail = self.tail?;
        unsafe {
            let tail_links = tail.links().as_mut();
            self.tail = tail_links.prev;

            if let Some(prev) = tail_links.prev {
                prev.links().as_mut().next = None;
            } else {
                self.head = None;
            }
            tail_links.unlink();
            Some(T::from_ptr(tail))
        }
    }

    /// Remove a node from the list.
    ///
    /// # Safety
    ///
    /// The caller *must* ensure that the removed node is an element of this
    /// linked list, and not any other linked list.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Ref> {
        let Links { next, prev } = item.links().as_mut().take();

        if let Some(prev) = prev {
            prev.links().as_mut().next = next;
        } else if self.head != Some(item) {
            return None;
        } else {
            self.head = next;
        }

        if let Some(next) = next {
            next.links().as_mut().prev = prev;
        } else if self.tail != Some(item) {
            return None;
        } else {
            self.tail = prev;
        }

        Some(T::from_ptr(item))
    }
}

unsafe impl<T: Link> Send for List<T> where T::Target: Send {}
unsafe impl<T: Link> Sync for List<T> where T::Target: Sync {}

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
