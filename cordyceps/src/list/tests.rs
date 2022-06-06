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
        assert_eq!(ll.len(), reference.len());
        ll.assert_valid();
    }
}
