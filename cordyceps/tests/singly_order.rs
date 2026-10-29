use cordyceps::{stack::Links, Linked, SortedList};
use std::{
    cell::Cell,
    cmp::Ordering,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::{self, NonNull},
    rc::Rc,
};

struct Entry {
    links: Links<Self>,
    id: usize,
    key: Rc<Cell<i32>>,
    drops: Rc<Cell<usize>>,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
unsafe impl Linked<Links<Self>> for Entry {
    type Handle = Box<Self>;
    fn into_ptr(handle: Box<Self>) -> NonNull<Self> {
        NonNull::from(Box::leak(handle))
    }
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Box<Self> {
        Box::from_raw(ptr.as_ptr())
    }
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<Links<Self>> {
        NonNull::new_unchecked(ptr::addr_of_mut!((*ptr.as_ptr()).links))
    }
}
fn entry(id: usize, key: i32, drops: &Rc<Cell<usize>>) -> Box<Entry> {
    Box::new(Entry {
        links: Links::new(),
        id,
        key: Rc::new(Cell::new(key)),
        drops: drops.clone(),
    })
}
fn ascending(a: &Entry, b: &Entry) -> Ordering {
    a.key.get().cmp(&b.key.get())
}
fn descending(a: &Entry, b: &Entry) -> Ordering {
    b.key.get().cmp(&a.key.get())
}
fn magnitude(a: &Entry, b: &Entry) -> Ordering {
    a.key.get().abs().cmp(&b.key.get().abs())
}

#[test]
fn assert_valid_rejects_changed_keys_under_the_lists_comparator() {
    for (cmp, head_key, other_key) in [
        (ascending as fn(&Entry, &Entry) -> Ordering, 99, -99),
        (descending, -99, 99),
        (magnitude, 99, 0),
    ] {
        for position in 0..4 {
            let drops = Rc::new(Cell::new(0));
            let mut list = SortedList::new_with_cmp(cmp);
            for (id, key) in [-1, 3, -5, 7].into_iter().enumerate() {
                list.insert(entry(id, key, &drops));
            }
            list.assert_valid();
            let ids: Vec<_> = list.iter().map(|e| e.id).collect();
            let key = list.iter().nth(position).unwrap().key.clone();
            let old = key.replace(if position == 0 { head_key } else { other_key });
            assert!(
                catch_unwind(AssertUnwindSafe(|| list.assert_valid())).is_err(),
                "missed disorder at position {position}"
            );
            assert_eq!(list.len(), 4);
            assert_eq!(drops.get(), 0);
            assert_eq!(list.iter().map(|e| e.id).collect::<Vec<_>>(), ids);
            key.set(old);
            list.assert_valid();
            drop(list);
            assert_eq!(drops.get(), 4);
        }
    }
}

#[test]
fn assert_valid_accepts_empty_singleton_and_equal_keys() {
    let drops = Rc::new(Cell::new(0));
    for cmp in [
        ascending as fn(&Entry, &Entry) -> Ordering,
        descending,
        magnitude,
    ] {
        let mut list = SortedList::new_with_cmp(cmp);
        list.assert_valid();
        list.insert(entry(0, 4, &drops));
        list.front().unwrap().key.set(-8);
        list.assert_valid();
        for id in 1..6 {
            list.insert(entry(id, -8, &drops));
        }
        list.assert_valid();
        assert_eq!(list.len(), 6);
    }
    assert_eq!(drops.get(), 18);
}

thread_local! { static BUDGET: Cell<usize> = const { Cell::new(usize::MAX) }; }
fn interrupted(a: &Entry, b: &Entry) -> Ordering {
    BUDGET.with(|budget| {
        let left = budget.get();
        assert!(left > 0, "comparator interrupted");
        budget.set(left - 1);
    });
    descending(a, b)
}

#[test]
fn interrupted_validation_preserves_every_linked_element() {
    for budget in 0..8 {
        let drops = Rc::new(Cell::new(0));
        BUDGET.with(|b| b.set(usize::MAX));
        let mut list = SortedList::new_with_cmp(interrupted);
        for id in 0..6 {
            list.insert(entry(id, id as i32, &drops));
        }
        let before: Vec<_> = list.iter().map(|e| e.id).collect();
        BUDGET.with(|b| b.set(budget));
        let _ = catch_unwind(AssertUnwindSafe(|| list.assert_valid()));
        BUDGET.with(|b| b.set(usize::MAX));
        list.assert_valid();
        assert_eq!(list.iter().map(|e| e.id).collect::<Vec<_>>(), before);
        assert_eq!(list.len(), 6);
        assert_eq!(drops.get(), 0);
        drop(list);
        assert_eq!(drops.get(), 6);
    }
}
