use super::Linked;
use crate::util::FmtOption;
use core::{
    fmt,
    marker::PhantomPinned,
    mem::ManuallyDrop,
    ptr::{self, NonNull},
};

pub struct List<T: ?Sized> {
    head: Option<NonNull<T>>,
    tail: Option<NonNull<T>>,
}

pub struct Links<T: ?Sized> {
    next: Option<NonNull<T>>,
    prev: Option<NonNull<T>>,
    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// https://github.com/rust-lang/rust/issues/63818.
    _unpin: PhantomPinned,
}

pub struct Cursor<'a, T: Linked<Links<T>> + ?Sized> {
    list: &'a mut List<T>,
    curr: Option<NonNull<T>>,
}

pub struct Iter<'a, T: Linked<Links<T>> + ?Sized> {
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
        // tracing::trace!(?self, ?ptr, "push_front");
        assert_ne!(self.head, Some(ptr));
        unsafe {
            let mut links = T::links(ptr);
            links.as_mut().next = self.head;
            links.as_mut().prev = None;
            // tracing::trace!(?links);
            if let Some(head) = self.head {
                T::links(head).as_mut().prev = Some(ptr);
                // tracing::trace!(head.links = ?T::links(head).as_ref(), "set head prev ptr",);
            }
        }

        self.head = Some(ptr);

        if self.tail.is_none() {
            self.tail = Some(ptr);
        }

        // tracing::trace!(?self, "push_front: pushed");
    }

    pub fn pop_back(&mut self) -> Option<T::Handle> {
        let tail = self.tail?;
        unsafe {
            let mut tail_links = T::links(tail);
            // tracing::trace!(?self, tail.addr = ?tail, tail.links = ?tail_links, "pop_back");
            self.tail = tail_links.as_ref().prev;
            debug_assert_eq!(
                tail_links.as_ref().next,
                None,
                "the tail node must not have a next link"
            );

            if let Some(prev) = tail_links.as_mut().prev {
                T::links(prev).as_mut().next = None;
            } else {
                self.head = None;
            }

            tail_links.as_mut().unlink();
            // tracing::trace!(?self, tail.links = ?tail_links, "pop_back: popped");
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
        let mut links = T::links(item);
        let links = links.as_mut();
        // tracing::trace!(?self, item.addr = ?item, item.links = ?links, "remove");
        let prev = links.prev.take();
        let next = links.next.take();

        if let Some(prev) = prev {
            T::links(prev).as_mut().next = next;
        } else if self.head != Some(item) {
            // tracing::trace!(?self.head, "item is not head, but has no prev; return None");
            return None;
        } else {
            debug_assert_ne!(Some(item), next, "node must not be linked to itself");
            self.head = next;
        }

        if let Some(next) = next {
            T::links(next).as_mut().prev = prev;
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

    pub fn cursor(&mut self) -> Cursor<'_, T> {
        Cursor {
            curr: self.head,
            list: self,
        }
    }

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
    pub const fn new() -> Self {
        Self {
            next: None,
            prev: None,
            _unpin: PhantomPinned,
        }
    }

    fn unlink(&mut self) {
        self.next = None;
        self.prev = None;
    }

    pub fn is_linked(&self) -> bool {
        self.next.is_some() || self.prev.is_some()
    }

    fn assert_valid(&self, head: &Self, tail: &Self)
    where
        T: Linked<Self>,
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

impl<T: ?Sized> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ?Sized> fmt::Debug for Links<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Links")
            .field("self", &format_args!("{:p}", self))
            .field("next", &FmtOption::new(&self.next))
            .field("prev", &FmtOption::new(&self.prev))
            .finish()
    }
}

impl<T: ?Sized> PartialEq for Links<T> {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next && self.prev == other.prev
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

impl<'a, T: Linked<Links<T>> + ?Sized> Iterator for Iter<'a, T> {
    type Item = T::Handle;
    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.curr.take()?;
        unsafe {
            self.curr = T::links(curr).as_ref().next;
            Some(T::from_ptr(curr))
        }
    }
}

#[cfg(test)]
#[cfg(not(loom))]
mod tests {
    use super::*;

    use std::pin::Pin;

    #[derive(Debug)]
    struct Entry<'a> {
        links: Links<Entry<'a>>,
        val: i32,
        _lt: std::marker::PhantomData<&'a ()>,
    }

    unsafe impl<'a> Linked<Links<Self>> for Entry<'a> {
        type Handle = Pin<&'a Entry<'a>>;

        fn as_ptr(handle: &Pin<&'a Entry>) -> NonNull<Entry<'a>> {
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

        unsafe fn links(mut target: NonNull<Entry<'a>>) -> NonNull<Links<Entry<'a>>> {
            NonNull::from(&mut target.as_mut().links)
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
            assert!($e.links.next.is_none());
            assert!($e.links.prev.is_none());
        }};
    }

    macro_rules! assert_ptr_eq {
        ($a:expr, $b:expr) => {{
            // Deal with mapping a Pin<&mut T> -> Option<NonNull<T>>
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

                assert!(b.links.next.is_none());
                assert!(b.links.prev.is_none());

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
                assert_ptr_eq!(c, b.links.next);
                assert_ptr_eq!(b, c.links.prev);

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

                assert_ptr_eq!(c, a.links.next);
                assert_ptr_eq!(a, c.links.prev);

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

                assert!(b.links.next.is_none());
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

                assert!(a.links.next.is_none());
                assert!(a.links.prev.is_none());

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

    #[test]
    fn cursor() {
        let _trace = trace_init();

        let a = entry(5);
        let b = entry(7);

        let mut list = List::<Entry<'_>>::new();

        assert_eq!(0, list.cursor().count());

        list.push_front(a.as_ref());
        list.push_front(b.as_ref());

        let mut i = list.cursor();
        assert_eq!(7, i.next().unwrap().val);
        assert_eq!(5, i.next().unwrap().val);
        assert!(i.next().is_none());
    }

    proptest::proptest! {
        #[test]
        fn fuzz_linked_list(ops: Vec<usize>) {
            let _trace = trace_init();
            let _span = tracing::info_span!("fuzz").entered();
            tracing::info!(?ops);
            run_fuzz(ops);
        }
    }

    fn run_fuzz(ops: Vec<usize>) {
        use std::collections::VecDeque;

        #[derive(Debug)]
        enum Op {
            Push,
            Pop,
            Remove(usize),
        }

        let ops = ops
            .iter()
            .map(|i| match i % 3 {
                0 => Op::Push,
                1 => Op::Pop,
                2 => Op::Remove(i / 3),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();

        let mut ll = List::<Entry<'_>>::new();
        let mut reference = VecDeque::new();

        let entries: Vec<_> = (0..ops.len()).map(|i| entry(i as i32)).collect();

        for (i, op) in ops.iter().enumerate() {
            let _span = tracing::info_span!("op", ?i, ?op).entered();
            match op {
                Op::Push => {
                    reference.push_front(i as i32);
                    assert_eq!(entries[i].val, i as i32);

                    ll.push_front(entries[i].as_ref());
                }
                Op::Pop => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        continue;
                    }

                    let v = reference.pop_back();
                    assert_eq!(v, ll.pop_back().map(|v| v.val));
                }
                Op::Remove(n) => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
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
