use super::*;

/// An entry type whose ownership is assigned to the list directly.
#[derive(Debug)]
#[pin_project::pin_project]
struct OwnedEntry {
    #[pin]
    links: Links<OwnedEntry>,
    val: i32,
}

unsafe impl Linked<Links<Self>> for OwnedEntry {
    type Handle = Pin<Box<OwnedEntry>>;

    fn into_ptr(handle: Pin<Box<OwnedEntry>>) -> NonNull<Self> {
        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        // Safety: if this function is only called by the linked list
        // implementation (and it is not intended for external use), we can
        // expect that the `NonNull` was constructed from a reference which
        // was pinned.
        //
        // If other callers besides `List`'s internals were to call this on
        // some random `NonNull<Entry>`, this would not be the case, and
        // this could be constructing an erroneous `Pin` from a referent
        // that may not be pinned!
        Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
    }

    unsafe fn links(target: NonNull<Self>) -> NonNull<Links<Self>> {
        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
        // Safety: it's fine to use `new_unchecked` here; if the pointer that we
        // offset to the `links` field is not null (which it shouldn't be, as we
        // received it as a `NonNull`), the offset pointer should therefore also
        // not be null.
        NonNull::new_unchecked(links)
    }
}

fn owned_entry(val: i32) -> Pin<Box<OwnedEntry>> {
    Box::pin(OwnedEntry {
        links: Links::new(),
        val,
    })
}

#[test]
fn pop_front() {
    let _trace = trace_init();

    let a = owned_entry(5);
    let b = owned_entry(7);
    let c = owned_entry(9);
    let mut list = List::<OwnedEntry>::new();

    list.push_front(a);
    list.assert_valid();

    list.push_front(b);
    list.assert_valid();

    list.push_front(c);
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
fn iterate_twice() {
    let _trace = trace_init();

    let a = owned_entry(1);
    let b = owned_entry(2);
    let c = owned_entry(3);
    let mut list = List::<OwnedEntry>::new();

    list.push_back(a);
    list.push_back(b);
    list.push_back(c);

    // iterate over the list...
    let mut i = 1;
    for entry in list.iter() {
        assert_eq!(entry.val, i);
        i += 1;
    }

    // do it again; it should still work!
    let mut i = 1;
    for entry in list.iter() {
        assert_eq!(entry.val, i);
        i += 1;
    }
}

#[test]
fn cursor_twice() {
    let _trace = trace_init();

    let a = owned_entry(1);
    let b = owned_entry(2);
    let c = owned_entry(3);
    let mut list = List::<OwnedEntry>::new();

    list.push_back(a);
    list.push_back(b);
    list.push_back(c);

    // iterate over the list...
    let mut i = 1;
    for entry in list.cursor_front_mut() {
        assert_eq!(entry.val, i);
        i += 1;
    }

    // do it again; it should still work!
    let mut i = 1;
    for entry in list.cursor_front_mut() {
        assert_eq!(entry.val, i);
        i += 1;
    }
}

// These tests don't work with the borrowed entry type, because we mutate
// entries through the iterator

#[test]
fn double_ended_iter_mut() {
    let a = owned_entry(1);
    let b = owned_entry(2);
    let c = owned_entry(3);
    fn incr_entry(entry: Pin<&mut OwnedEntry>) -> i32 {
        let entry = entry.project();
        *entry.val += 1;
        *entry.val
    }

    let mut list = List::new();

    list.push_back(a);
    list.push_back(b);
    list.push_back(c);

    let head_to_tail = list.iter_mut().map(incr_entry).collect::<Vec<_>>();
    assert_eq!(&head_to_tail, &[2, 3, 4]);

    let tail_to_head = list.iter_mut().rev().map(incr_entry).collect::<Vec<_>>();
    assert_eq!(&tail_to_head, &[5, 4, 3]);
}

/// Per the double-ended iterator docs:
///
/// > It is important to note that both back and forth work on the same range,
/// > and do not cross: iteration is over when they meet in the middle.
#[test]
fn double_ended_iter_mut_empties() {
    let a = owned_entry(1);
    let b = owned_entry(2);
    let c = owned_entry(3);
    let d = owned_entry(4);

    let mut list = List::<OwnedEntry>::new();

    list.push_back(a);
    list.push_back(b);
    list.push_back(c);
    list.push_back(d);

    let mut iter = list.iter_mut();

    assert_eq!(iter.next().map(|entry| entry.val), Some(1));
    assert_eq!(iter.next().map(|entry| entry.val), Some(2));

    assert_eq!(iter.next_back().map(|entry| entry.val), Some(4));
    assert_eq!(iter.next_back().map(|entry| entry.val), Some(3));

    assert_eq!(iter.next().map(|entry| entry.val), None);
    assert_eq!(iter.next_back().map(|entry| entry.val), None);
}

#[test]
fn drain_filter() {
    let mut list = List::new();
    list.push_back(owned_entry(1));
    list.push_back(owned_entry(2));
    list.push_back(owned_entry(3));
    list.push_back(owned_entry(4));

    {
        // Create a scope so that the mutable borrow on the list is released
        // when we're done with the `drain_filter` iterator.
        let mut df = list.drain_filter(|entry: &OwnedEntry| entry.val % 2 == 0);
        assert_eq!(df.next().map(|entry| entry.val), Some(2));
        assert_eq!(df.next().map(|entry| entry.val), Some(4));
        assert_eq!(df.next().map(|entry| entry.val), None);
    }

    let remaining = list.iter().map(|entry| entry.val).collect::<Vec<_>>();
    assert_eq!(remaining, vec![1, 3]);
}
