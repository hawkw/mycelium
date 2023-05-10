use super::*;
use std::{
    boxed::Box,
    pin::Pin,
    ptr::{self, NonNull},
    vec,
    vec::Vec,
};

#[derive(Debug)]
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
        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
        // Safety: it's fine to use `new_unchecked` here; if the pointer that we
        // offset to the `links` field is not null (which it shouldn't be, as we
        // received it as a `NonNull`), the offset pointer should therefore also
        // not be null.
        NonNull::new_unchecked(links)
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

fn val(entry: Option<Pin<&Entry<'_>>>) -> Option<i32> {
    entry.map(|entry| entry.val)
}

fn drain_list(list: &mut List<Entry<'_>>) -> Vec<i32> {
    let mut ret = vec![];

    while let Some(entry) = list.pop_back() {
        ret.push(entry.val);
    }

    ret
}

fn collect_vals(list: &List<Entry<'_>>) -> Vec<i32> {
    list.iter().map(|entry| entry.val).collect::<Vec<_>>()
}

fn push_all<'a>(
    list: &mut List<Entry<'a>>,
    entries: impl IntoIterator<Item = &'a Pin<Box<Entry<'a>>>>,
) {
    list.extend(entries.into_iter().map(Pin::as_ref))
}

fn list_from_iter<'a>(
    entries: impl IntoIterator<Item = &'a Pin<Box<Entry<'a>>>>,
) -> List<Entry<'a>> {
    let mut list = List::new();
    push_all(&mut list, entries);
    list
}

macro_rules! assert_clean {
    ($e:ident) => {{
        assert!(!$e.links.is_linked())
    }};
}

macro_rules! assert_valid {
    ($e:ident) => {{
        $e.assert_valid_named(concat!("[", stringify!($e), "]: "));
    }};
}

macro_rules! assert_ptr_eq {
    ($a:expr, $b:expr) => {{
        // Deal with mapping a Pin<&mut T> -> Link<T>
        assert_eq!(Some($a.as_ref().get_ref().into()), $b)
    }};
}

mod cursor;
mod owned_entry;
mod remove_by_addr;

#[test]
fn const_new() {
    const _: List<Entry> = List::new();
}

fn trace_init() -> impl Drop {
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
    assert_valid!(list);
    list.push_front(b.as_ref());
    assert_valid!(list);
    list.push_front(c.as_ref());
    assert_valid!(list);

    let items: Vec<i32> = drain_list(&mut list);
    assert_eq!([5, 7, 31].to_vec(), items);

    assert_valid!(list);
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
    assert_valid!(list);

    list.push_front(b.as_ref());
    assert_valid!(list);

    list.push_front(c.as_ref());
    assert_valid!(list);

    let d = list.pop_front().unwrap();
    assert_eq!(9, d.val);

    let e = list.pop_front().unwrap();
    assert_eq!(7, e.val);

    let f = list.pop_front().unwrap();
    assert_eq!(5, f.val);

    assert!(list.is_empty());
    assert!(list.pop_front().is_none());
    assert_valid!(list);
}

#[test]
fn push_back() {
    let _trace = trace_init();

    let a = entry(5);
    let b = entry(7);
    let c = entry(9);
    let mut list = List::<Entry>::new();

    list.push_back(a.as_ref());
    assert_valid!(list);

    list.push_back(b.as_ref());
    assert_valid!(list);

    list.push_back(c.as_ref());
    assert_valid!(list);

    let d = list.pop_back().unwrap();
    assert_eq!(9, d.val);

    let e = list.pop_back().unwrap();
    assert_eq!(7, e.val);

    let f = list.pop_back().unwrap();
    assert_eq!(5, f.val);

    assert!(list.is_empty());
    assert!(list.pop_back().is_none());

    assert_valid!(list);
}

#[test]
fn push_pop_push_pop() {
    let _trace = trace_init();

    let a = entry(5);
    let b = entry(7);

    let mut list = List::<Entry>::new();

    list.push_front(a.as_ref());
    assert_valid!(list);

    let entry = list.pop_back().unwrap();
    assert_eq!(5, entry.val);
    assert!(list.is_empty());
    assert_valid!(list);

    list.push_front(b.as_ref());
    assert_valid!(list);

    let entry = list.pop_back().unwrap();
    assert_eq!(7, entry.val);
    assert_valid!(list);

    assert!(list.is_empty());
    assert!(list.pop_back().is_none());
    assert_valid!(list);
}

#[test]
fn double_ended_iter() {
    let entries = [entry(1), entry(2), entry(3)];
    let list = list_from_iter(&entries);

    let head_to_tail = list.iter().map(|entry| entry.val).collect::<Vec<_>>();
    assert_eq!(&head_to_tail, &[1, 2, 3]);

    let tail_to_head = list.iter().rev().map(|entry| entry.val).collect::<Vec<_>>();
    assert_eq!(&tail_to_head, &[3, 2, 1]);
}

/// Per the double-ended iterator docs:
///
/// > It is important to note that both back and forth work on the same range,
/// > and do not cross: iteration is over when they meet in the middle.
#[test]
fn double_ended_iter_empties() {
    let entries = [entry(1), entry(2), entry(3), entry(4)];

    let list = list_from_iter(&entries);

    let mut iter = list.iter();

    assert_eq!(iter.next().map(|entry| entry.val), Some(1));
    assert_eq!(iter.next().map(|entry| entry.val), Some(2));

    assert_eq!(iter.next_back().map(|entry| entry.val), Some(4));
    assert_eq!(iter.next_back().map(|entry| entry.val), Some(3));

    assert_eq!(iter.next().map(|entry| entry.val), None);
    assert_eq!(iter.next_back().map(|entry| entry.val), None);
}

#[test]
fn drain_filter() {
    let entries = [entry(1), entry(2), entry(3), entry(4)];
    let mut list = list_from_iter(&entries);

    {
        // Create a scope so that the mutable borrow on the list is released
        // when we're done with the `drain_filter` iterator.
        let mut df = list.drain_filter(|entry| entry.val % 2 == 0);
        assert_eq!(df.next().map(|entry| entry.val), Some(2));
        assert_eq!(df.next().map(|entry| entry.val), Some(4));
        assert_eq!(df.next().map(|entry| entry.val), None);
    }

    let remaining = list.iter().map(|entry| entry.val).collect::<Vec<_>>();
    assert_eq!(remaining, vec![1, 3]);
}

// #[test]
// fn cursor() {
//     let _trace = trace_init();

//     let a = entry(5);
//     let b = entry(7);

//     let mut list = List::<Entry<'_>>::new();

//     assert_eq!(0, list.cursor_front_mut().count());

//     list.push_front(a.as_ref());
//     list.push_front(b.as_ref());

//     let mut i = list.cursor_front_mut();
//     assert_eq!(7, i.next().unwrap().val);
//     assert_eq!(5, i.next().unwrap().val);
//     assert!(i.next().is_none());
// }

// Based (loosely) on the tests for `std::collections::LinkedList::append`:
// https://github.com/rust-lang/rust/blob/67404f7200c13deec255ffe1146e1d2c9d0d3028/library/alloc/src/collections/linked_list/tests.rs#L101-L156
mod append {
    use super::*;

    #[test]
    fn empty_to_empty() {
        let mut a = List::<Entry<'_>>::new();
        let mut b = List::new();
        a.append(&mut b);

        assert_valid!(a);
        assert_valid!(b);

        assert_eq!(a.len(), 0);
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn nonempty_to_empty() {
        let entry = entry(1);

        let mut a = List::<Entry<'_>>::new();
        let mut b = List::new();
        b.push_back(entry.as_ref());

        a.append(&mut b);

        assert_valid!(a);
        assert_valid!(b);

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 0);

        assert_eq!(val(a.front()), Some(1));
        assert_eq!(val(a.back()), Some(1));
    }

    #[test]
    fn empty_to_nonempty() {
        let entry = entry(1);

        let mut a = List::<Entry<'_>>::new();
        let mut b = List::new();
        a.push_back(entry.as_ref());

        a.append(&mut b);

        assert_valid!(a);
        assert_valid!(b);

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 0);

        assert_eq!(val(a.front()), Some(1));
        assert_eq!(val(a.back()), Some(1));
    }

    #[test]
    fn nonempty_to_nonempty() {
        let a_entries = [entry(1), entry(2), entry(3), entry(4), entry(5)];
        let b_entries = [
            entry(9),
            entry(8),
            entry(1),
            entry(2),
            entry(3),
            entry(4),
            entry(5),
        ];
        let three = entry(3);

        let mut a = list_from_iter(&a_entries);
        let mut b = list_from_iter(&b_entries);

        a.append(&mut b);

        assert_valid!(a);
        assert_valid!(b);

        assert_eq!(a.len(), a_entries.len() + b_entries.len());
        assert_eq!(b.len(), 0);

        let expected = a_entries
            .iter()
            .map(|entry| entry.val)
            .chain(b_entries.iter().map(|entry| entry.val));
        for item in expected {
            assert_eq!(val(a.pop_front()), Some(item));
        }

        // make sure `b` wasn't broken by being messed with...
        b.push_back(three.as_ref());
        assert_valid!(b);
        assert_eq!(b.len(), 1);
        assert_eq!(val(b.front()), Some(3));
        assert_eq!(val(b.back()), Some(3));
        assert_valid!(b);
    }
}

mod split_off {
    use super::*;

    #[test]
    fn single_node() {
        let entry = entry(1);

        let mut list = List::<Entry<'_>>::new();
        list.push_back(entry.as_ref());

        let split = list.split_off(0);
        assert_eq!(list.len(), 0);
        assert_eq!(split.len(), 1);
        assert_valid!(list);
        assert_valid!(split);

        assert_eq!(val(split.front()), Some(1));
        assert_eq!(val(split.back()), Some(1));
        assert_eq!(val(list.front()), None);
        assert_eq!(val(list.back()), None);
    }

    #[test]
    fn middle() {
        let entries = [entry(1), entry(2), entry(3), entry(4), entry(5)];

        let mut list = list_from_iter(&entries);
        let mut split = list.split_off(2);

        assert_eq!(list.len(), 2);
        assert_eq!(split.len(), 3);
        assert_valid!(list);
        assert_valid!(split);

        for n in 1..3 {
            assert_eq!(val(list.pop_front()), Some(n));
        }
        for n in 3..6 {
            assert_eq!(val(split.pop_front()), Some(n));
        }
    }

    #[test]
    fn one_node() {
        let entries = [entry(1), entry(2), entry(3), entry(4), entry(5)];

        let mut list = list_from_iter(&entries);
        let mut split = list.split_off(4);

        assert_eq!(list.len(), 4);
        assert_eq!(split.len(), 1);
        assert_valid!(list);
        assert_valid!(split);

        for n in 1..5 {
            assert_eq!(val(list.pop_front()), Some(n));
        }
        for n in 5..6 {
            assert_eq!(val(split.pop_front()), Some(n));
        }
    }

    #[test]
    fn last_index() {
        let one = entry(1);

        let mut list = List::<Entry<'_>>::new();
        list.push_back(one.as_ref());

        let split = list.split_off(1);
        assert_eq!(list.len(), 1);
        assert_eq!(split.len(), 0);

        assert_valid!(list);
        assert_valid!(split);

        assert_eq!(val(list.front()), Some(1));
        assert_eq!(val(list.back()), Some(1));
        assert_eq!(val(split.front()), None);
        assert_eq!(val(split.back()), None);
    }

    #[test]
    fn all_splits() {
        let _trace = trace_init();
        let entries = [entry(1), entry(2), entry(3), entry(4), entry(5)];
        let vals = [1, 2, 3, 4, 5];
        let mut list = list_from_iter(&entries);

        // test all splits
        for i in 0..1 + list.len() {
            tracing::info!(at = i, "test split");

            // split off at this index
            let mut split = list.split_off(i);
            tracing::info!(?split, ?list);
            assert_valid!(list);
            assert_valid!(split);

            let split_entries = split.iter().map(|entry| entry.val).collect::<Vec<_>>();
            assert_eq!(split_entries, vals[i..]);

            // and put them back together
            list.extend(split.drain_filter(|_| true));
            let list_entries = list.iter().map(|entry| entry.val).collect::<Vec<_>>();
            assert_eq!(list_entries, vals[..])
        }
    }
}

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

    let entries: Vec<_> = (0..ops.len()).map(|i| entry(i as i32)).collect();
    let mut ll = List::<Entry<'_>>::new();
    let mut reference = VecDeque::new();

    for (i, op) in ops.iter().enumerate() {
        let _span = tracing::info_span!("op", ?i, ?op).entered();
        tracing::info!(?reference);
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
        assert_eq!(ll.len(), reference.len());
        ll.assert_valid();
    }
}
