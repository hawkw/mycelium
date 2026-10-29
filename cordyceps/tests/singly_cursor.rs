//! The length, removal and bulk operations on the singly-linked collections.
use cordyceps::{stack::Links, Linked, SortedList, Stack, TransferStack};
use std::{
    cell::Cell,
    cmp::Ordering,
    panic,
    pin::Pin,
    ptr::{self, NonNull},
    rc::Rc,
};

/// Counts the elements which have actually been dropped, so that a list which
/// loses an element rather than handing it back is caught.
type Drops = Rc<Cell<usize>>;

struct Entry {
    links: Links<Entry>,
    val: i32,
    drops: Drops,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.val == other.val
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.val.cmp(&other.val)
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry").field("val", &self.val).finish()
    }
}

unsafe impl Linked<Links<Self>> for Entry {
    type Handle = Box<Entry>;

    fn into_ptr(handle: Box<Entry>) -> NonNull<Entry> {
        NonNull::from(Box::leak(handle))
    }

    unsafe fn from_ptr(ptr: NonNull<Entry>) -> Box<Entry> {
        Box::from_raw(ptr.as_ptr())
    }

    unsafe fn links(target: NonNull<Entry>) -> NonNull<Links<Entry>> {
        NonNull::new_unchecked(ptr::addr_of_mut!((*target.as_ptr()).links))
    }
}

fn counter() -> Drops {
    Rc::new(Cell::new(0))
}

fn entry(drops: &Drops, val: i32) -> Box<Entry> {
    Box::new(Entry {
        links: Links::new(),
        val,
        drops: drops.clone(),
    })
}

/// The address of an element, taken before it is handed to a list. A `Box`'s
/// allocation does not move when the `Box` does, so this stays valid.
fn addr(e: &Box<Entry>) -> NonNull<Entry> {
    NonNull::from(&**e)
}

fn val(e: Option<Pin<&Entry>>) -> Option<i32> {
    e.map(|e| e.val)
}

fn drain_stack(stack: &mut Stack<Entry>) -> Vec<i32> {
    let mut out = Vec::new();
    while let Some(e) = stack.pop() {
        out.push(e.val);
    }
    out
}

fn drain_sorted(list: &mut SortedList<Entry>) -> Vec<i32> {
    let mut out = Vec::new();
    while let Some(e) = list.pop_front() {
        out.push(e.val);
    }
    out
}

fn sorted_of(drops: &Drops, vals: &[i32]) -> SortedList<Entry> {
    let mut list = SortedList::new_min();
    for &v in vals {
        list.insert(entry(drops, v));
    }
    list
}

/// Runs `f`, reporting whether it panicked, without the panic reaching the test
/// runner's output.
fn panicked(f: impl FnOnce()) -> bool {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let out = panic::catch_unwind(panic::AssertUnwindSafe(f));
    panic::set_hook(hook);
    out.is_err()
}

thread_local! {
    /// How many times `Linked::links` has been reached for a `Counted`.
    static LINK_READS: Cell<usize> = const { Cell::new(0) };
}

fn link_reads() -> usize {
    LINK_READS.with(|c| c.get())
}

struct Counted {
    links: Links<Counted>,
    val: i32,
}

impl PartialEq for Counted {
    fn eq(&self, other: &Self) -> bool {
        self.val == other.val
    }
}
impl Eq for Counted {}
impl PartialOrd for Counted {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Counted {
    fn cmp(&self, other: &Self) -> Ordering {
        self.val.cmp(&other.val)
    }
}

unsafe impl Linked<Links<Self>> for Counted {
    type Handle = Box<Counted>;

    fn into_ptr(handle: Box<Counted>) -> NonNull<Counted> {
        NonNull::from(Box::leak(handle))
    }

    unsafe fn from_ptr(ptr: NonNull<Counted>) -> Box<Counted> {
        Box::from_raw(ptr.as_ptr())
    }

    unsafe fn links(target: NonNull<Counted>) -> NonNull<Links<Counted>> {
        LINK_READS.with(|c| c.set(c.get() + 1));
        NonNull::new_unchecked(ptr::addr_of_mut!((*target.as_ptr()).links))
    }
}

fn counted(val: i32) -> Box<Counted> {
    Box::new(Counted {
        links: Links::new(),
        val,
    })
}

#[test]
fn a_cursor_starts_at_the_front_and_walks_forwards() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=4 {
        stack.push(entry(&drops, v));
    }

    let mut cursor = stack.cursor_front_mut();
    assert_eq!(val(cursor.current()), Some(4), "it starts on the front");
    assert_eq!(cursor.index(), Some(0));
    assert_eq!(val(cursor.peek_next()), Some(3), "and can see the next one");

    for (step, expected) in [(1, 3), (2, 2), (3, 1)] {
        cursor.move_next();
        assert_eq!(val(cursor.current()), Some(expected));
        assert_eq!(cursor.index(), Some(step));
    }
    assert_eq!(
        val(cursor.peek_next()),
        None,
        "nothing follows the last one"
    );

    drop(cursor);
    assert_eq!(stack.len(), 4, "walking took nothing out");
    stack.assert_valid();
    assert_eq!(drops.get(), 0);

    // the same over a sorted list, which the cursor walks in sorted order
    let mut list = sorted_of(&drops, &[9, 1, 5]);
    let mut cursor = list.cursor_front_mut();
    assert_eq!(val(cursor.current()), Some(1));
    cursor.move_next();
    assert_eq!(val(cursor.current()), Some(5));
    cursor.move_next();
    assert_eq!(val(cursor.current()), Some(9));
    assert_eq!(cursor.index(), Some(2));
    drop(cursor);
    assert_eq!(list.len(), 3);
    list.assert_valid();
}

#[test]
fn a_cursor_steps_off_the_end_and_round_to_the_front() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=2 {
        stack.push(entry(&drops, v));
    }

    let mut cursor = stack.cursor_front_mut();
    cursor.move_next();
    cursor.move_next();
    assert_eq!(
        val(cursor.current()),
        None,
        "past the last one it holds none"
    );
    assert_eq!(cursor.index(), None, "and is nowhere in particular");
    assert_eq!(
        val(cursor.peek_next()),
        Some(2),
        "the one after nowhere is the front"
    );

    cursor.move_next();
    assert_eq!(val(cursor.current()), Some(2), "and moving on comes round");
    assert_eq!(cursor.index(), Some(0));

    // there it takes nothing out and gives nothing back
    let mut empty = SortedList::<Entry>::new_min();
    let mut cursor = empty.cursor_front_mut();
    assert_eq!(
        val(cursor.current()),
        None,
        "an empty list has nothing to be on"
    );
    assert_eq!(cursor.index(), None);
    assert!(cursor.remove_current().is_none());
    cursor.move_next();
    assert_eq!(
        val(cursor.current()),
        None,
        "and coming round finds nothing"
    );
    drop(cursor);
    assert_eq!(empty.len(), 0);
    empty.assert_valid();
}

#[test]
fn a_cursor_takes_out_the_element_it_is_on() {
    // the front, where the collection's own head has to move
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=4 {
        stack.push(entry(&drops, v));
    }
    let mut cursor = stack.cursor_front_mut();
    let taken = cursor.remove_current().expect("it is on the front");
    assert_eq!(taken.val, 4);
    assert_eq!(
        val(cursor.current()),
        Some(3),
        "and is left on what followed"
    );
    assert_eq!(cursor.index(), Some(0), "which is now the front");
    drop(cursor);
    drop(taken);
    assert_eq!(stack.len(), 3);
    stack.assert_valid();
    assert_eq!(drain_stack(&mut stack), vec![3, 2, 1]);

    // one in the middle, where the element before it has to be pointed past
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=4 {
        stack.push(entry(&drops, v));
    }
    let mut cursor = stack.cursor_front_mut();
    cursor.move_next();
    let taken = cursor.remove_current().expect("it is on the second");
    assert_eq!(taken.val, 3);
    assert_eq!(val(cursor.current()), Some(2));
    assert_eq!(cursor.index(), Some(1));
    drop(cursor);
    drop(taken);
    assert_eq!(stack.len(), 3);
    stack.assert_valid();
    assert_eq!(drain_stack(&mut stack), vec![4, 2, 1]);

    // and the last one, after which there is nowhere to be
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3]);
    let mut cursor = list.cursor_front_mut();
    cursor.move_next();
    cursor.move_next();
    let taken = cursor.remove_current().expect("it is on the last");
    assert_eq!(taken.val, 3);
    assert_eq!(val(cursor.current()), None, "nothing followed it");
    assert_eq!(cursor.index(), None);
    assert!(
        cursor.remove_current().is_none(),
        "and there is nothing left to take"
    );
    drop(cursor);
    drop(taken);
    assert_eq!(list.len(), 2);
    list.assert_valid();
    assert_eq!(drain_sorted(&mut list), vec![1, 2]);
}

#[test]
fn what_a_cursor_takes_out_belongs_to_the_caller() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=5 {
        stack.push(entry(&drops, v));
    }

    // walking and removing as it goes reaches every element, because after a
    // removal the cursor is on the one which followed rather than past it.
    let mut moved = SortedList::<Entry>::new_min();
    {
        let mut cursor = stack.cursor_front_mut();
        while let Some(e) = cursor.remove_current() {
            moved.insert(e);
        }
        assert_eq!(cursor.current(), None);
    }

    assert_eq!(
        stack.len(),
        0,
        "the stack was emptied one element at a time"
    );
    assert!(stack.is_empty());
    stack.assert_valid();
    assert_eq!(drops.get(), 0, "and nothing was dropped on the way");

    // what came out was detached, so the list it went into is well formed
    moved.assert_valid();
    assert_eq!(moved.len(), 5, "every element crossed over");
    assert_eq!(drain_sorted(&mut moved), vec![1, 2, 3, 4, 5]);

    // a cursor abandoned part way leaves everything it did not take
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3, 4]);
    {
        let mut cursor = list.cursor_front_mut();
        drop(cursor.remove_current());
        cursor.move_next();
    }
    assert_eq!(list.len(), 3, "only what it took is gone");
    assert_eq!(drops.get(), 1);
    list.assert_valid();
    assert_eq!(drain_sorted(&mut list), vec![2, 3, 4]);
}

#[test]
fn a_cursor_puts_an_element_in_after_the_one_it_is_on() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=3 {
        stack.push(entry(&drops, v));
    }

    let mut cursor = stack.cursor_front_mut();
    cursor.insert_after(entry(&drops, 9));
    assert_eq!(
        val(cursor.current()),
        Some(3),
        "the cursor stays where it was"
    );
    assert_eq!(
        val(cursor.peek_next()),
        Some(9),
        "and the new one follows it"
    );
    drop(cursor);
    assert_eq!(stack.len(), 4);
    stack.assert_valid();

    // off the stack the element goes to the front, as `List`'s cursor does
    let mut cursor = stack.cursor_front_mut();
    for _ in 0..4 {
        cursor.move_next();
    }
    assert_eq!(val(cursor.current()), None, "off the stack");
    cursor.insert_after(entry(&drops, 7));
    assert_eq!(val(cursor.peek_next()), Some(7), "which is now the front");
    drop(cursor);
    assert_eq!(stack.len(), 5);
    stack.assert_valid();
    assert_eq!(drain_stack(&mut stack), vec![7, 3, 9, 2, 1]);
    assert_eq!(drops.get(), 5, "and nothing was lost");

    // inserting at the very end puts it last
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    stack.push(entry(&drops, 1));
    let mut cursor = stack.cursor_front_mut();
    cursor.insert_after(entry(&drops, 2));
    drop(cursor);
    assert_eq!(stack.len(), 2);
    stack.assert_valid();
    assert_eq!(drain_stack(&mut stack), vec![1, 2]);
}

#[test]
fn a_cursor_splits_a_collection_where_it_stands() {
    // at each position in turn, the split keeps everything up to and including
    // the element the cursor is on
    for at in 0..4 {
        let drops = counter();
        let mut list = sorted_of(&drops, &[1, 2, 3, 4]);
        let rest = {
            let mut cursor = list.cursor_front_mut();
            for _ in 0..at {
                cursor.move_next();
            }
            cursor.split_after()
        };
        let mut rest = rest;
        list.assert_valid();
        rest.assert_valid();
        assert_eq!(list.len(), at + 1, "splitting after position {at}");
        assert_eq!(rest.len(), 3 - at);
        assert_eq!(drops.get(), 0, "splitting after {at} dropped something");

        let mut seen = drain_sorted(&mut list);
        seen.extend(drain_sorted(&mut rest));
        assert_eq!(seen, vec![1, 2, 3, 4], "splitting after {at} lost order");
    }

    // off the list, everything moves
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3]);
    let mut all = {
        let mut cursor = list.cursor_front_mut();
        for _ in 0..3 {
            cursor.move_next();
        }
        assert_eq!(val(cursor.current()), None);
        cursor.split_after()
    };
    assert_eq!(list.len(), 0, "nothing was left behind");
    assert!(list.is_empty());
    list.assert_valid();
    assert_eq!(all.len(), 3);
    all.assert_valid();

    // the half which moved is ordered by the function its parent was built
    // with, so inserting into it afterwards still lands correctly
    fn by_distance_from_zero(a: &Entry, b: &Entry) -> Ordering {
        a.val.abs().cmp(&b.val.abs())
    }
    let drops = counter();
    let mut parent = SortedList::<Entry>::new_with_cmp(by_distance_from_zero);
    for v in [-1, 4, -7, 9] {
        parent.insert(entry(&drops, v));
    }
    let mut tail = {
        let mut cursor = parent.cursor_front_mut();
        cursor.move_next();
        cursor.split_after()
    };
    parent.insert(entry(&drops, -2));
    tail.insert(entry(&drops, 8));
    parent.assert_valid();
    tail.assert_valid();
    assert_eq!(drain_sorted(&mut parent), vec![-1, -2, 4]);
    assert_eq!(drain_sorted(&mut tail), vec![-7, 8, 9]);

    // and a stack splits the same way
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=4 {
        stack.push(entry(&drops, v));
    }
    let mut rest = {
        let mut cursor = stack.cursor_front_mut();
        cursor.move_next();
        cursor.split_after()
    };
    stack.assert_valid();
    rest.assert_valid();
    assert_eq!(stack.len(), 2);
    assert_eq!(rest.len(), 2);
    assert_eq!(drain_stack(&mut stack), vec![4, 3]);
    assert_eq!(drain_stack(&mut rest), vec![2, 1]);
}
#[test]
fn stack_cursor_edits_compose_without_losing_position_or_ownership() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    let mut model: Vec<i32> = (0..48).rev().collect();
    for value in 0..48 {
        stack.push(entry(&drops, value));
    }
    let mut created = 48;
    let mut received = SortedList::<Entry>::new_min();
    let mut position = Some(0);
    let mut seed = 713_u64;
    {
        let mut cursor = stack.cursor_front_mut();
        for step in 0..600 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            match (seed >> 32) % 5 {
                0 => {
                    cursor.move_next();
                    position = match position {
                        Some(i) if i + 1 < model.len() => Some(i + 1),
                        Some(_) => None,
                        None if !model.is_empty() => Some(0),
                        None => None,
                    };
                }
                1 | 2 => {
                    cursor.insert_after(entry(&drops, created));
                    model.insert(position.map_or(0, |i| i + 1), created);
                    created += 1;
                }
                3 => {
                    let removed = cursor.remove_current();
                    if let Some(i) = position {
                        let expected = model.remove(i);
                        let handle = removed.expect("current element must be returned");
                        assert_eq!(handle.val, expected);
                        received.insert(handle);
                        if i == model.len() {
                            position = None;
                        }
                    } else {
                        assert!(removed.is_none());
                    }
                }
                _ => {
                    let expected = model.split_off(position.map_or(0, |i| i + 1));
                    let mut suffix = cursor.split_after();
                    assert_eq!(suffix.len(), expected.len(), "step {step}");
                    suffix.assert_valid();
                    for value in expected {
                        let handle = suffix.pop().expect("split retained every element");
                        assert_eq!(handle.val, value);
                        received.insert(handle);
                    }
                    assert!(suffix.is_empty());
                }
            }
            assert_eq!(cursor.index(), position, "step {step}");
            assert_eq!(
                val(cursor.current()),
                position.map(|i| model[i]),
                "step {step}"
            );
            assert_eq!(
                val(cursor.peek_next()),
                model.get(position.map_or(0, |i| i + 1)).copied(),
                "step {step}"
            );
        }
    }
    stack.assert_valid();
    received.assert_valid();
    assert_eq!(stack.len(), model.len());
    assert_eq!(stack.len() + received.len() + drops.get(), created as usize);
    assert_eq!(drain_stack(&mut stack), model);
    drop(received);
    assert_eq!(drops.get(), created as usize);
}

#[test]
fn sorted_cursor_removal_split_and_reuse_preserve_the_comparator() {
    fn by_magnitude(a: &Entry, b: &Entry) -> Ordering {
        a.val.abs().cmp(&b.val.abs())
    }
    for at in 0..=8 {
        let drops = counter();
        let mut list = SortedList::new_with_cmp(by_magnitude);
        let mut model = vec![-1, 3, -5, 7, -9, 11, -13, 15];
        for &value in &model {
            list.insert(entry(&drops, value));
        }
        let mut taken = Stack::<Entry>::new();
        let mut cursor = list.cursor_front_mut();
        for _ in 0..at {
            cursor.move_next();
        }
        for _ in 0..2 {
            let handle = cursor.remove_current();
            if at < model.len() {
                let expected = model.remove(at);
                assert_eq!(handle.as_ref().map(|e| e.val), Some(expected));
                taken.push(handle.unwrap());
            } else {
                assert!(handle.is_none());
            }
            assert_eq!(cursor.index(), (at < model.len()).then_some(at));
        }
        let position = (at < model.len()).then_some(at);
        let mut suffix_model = model.split_off(position.map_or(0, |i| i + 1));
        let mut suffix = cursor.split_after();
        assert_eq!(cursor.index(), position);
        assert_eq!(val(cursor.current()), position.map(|i| model[i]));
        assert!(cursor.peek_next().is_none());
        let empty = cursor.split_after();
        assert!(empty.is_empty());
        cursor.move_next();
        let after = if position.is_none() && !model.is_empty() {
            Some(0)
        } else {
            None
        };
        assert_eq!(cursor.index(), after);
        cursor.move_next();
        assert_eq!(
            cursor.index(),
            if model.is_empty() { None } else { Some(0) }
        );
        drop(cursor);

        list.insert(entry(&drops, -4));
        suffix.insert(entry(&drops, -12));
        model.push(-4);
        suffix_model.push(-12);
        model.sort_by_key(|v| v.abs());
        suffix_model.sort_by_key(|v| v.abs());
        assert_eq!(list.iter().map(|e| e.val).collect::<Vec<_>>(), model);
        assert_eq!(
            suffix.iter().map(|e| e.val).collect::<Vec<_>>(),
            suffix_model
        );
        list.assert_valid();
        suffix.assert_valid();
        assert_eq!(list.len() + suffix.len() + taken.len() + drops.get(), 10);
        list.merge(&mut suffix);
        while let Some(handle) = taken.pop() {
            list.insert(handle);
        }
        list.assert_valid();
        assert_eq!(list.len(), 10);
        assert_eq!(
            drain_sorted(&mut list),
            [-1, 3, -4, -5, 7, -9, 11, -12, -13, 15]
        );
        assert_eq!(drops.get(), 10);
    }
}

#[test]
fn cursor_work_does_not_grow_with_distance_or_suffix_length() {
    fn measure(size: i32) -> Vec<usize> {
        let mut stack = Stack::<Counted>::new();
        let mut list = SortedList::<Counted>::new_min();
        for value in 0..size {
            stack.push(counted(value));
        }
        for value in (0..size).rev() {
            list.insert(counted(value));
        }
        let mut work = Vec::new();
        macro_rules! measured {
            ($operation:expr) => {{
                let before = link_reads();
                let result = $operation;
                work.push(link_reads() - before);
                result
            }};
        }
        {
            let mut cursor = stack.cursor_front_mut();
            for _ in 0..size / 2 {
                cursor.move_next();
            }
            assert_eq!(measured!(cursor.index()), Some(size as usize / 2));
            assert!(measured!(cursor.current()).is_some());
            assert!(measured!(cursor.peek_next()).is_some());
            measured!(cursor.move_next());
            let handle = measured!(cursor.remove_current()).unwrap();
            measured!(cursor.insert_after(handle));
            let suffix = measured!(cursor.split_after());
            drop(suffix);
        }
        {
            let mut cursor = list.cursor_front_mut();
            for _ in 0..size / 2 {
                cursor.move_next();
            }
            assert_eq!(measured!(cursor.index()), Some(size as usize / 2));
            assert!(measured!(cursor.current()).is_some());
            assert!(measured!(cursor.peek_next()).is_some());
            measured!(cursor.move_next());
            let handle = measured!(cursor.remove_current()).unwrap();
            drop(handle);
            let suffix = measured!(cursor.split_after());
            drop(suffix);
        }
        stack.assert_valid();
        list.assert_valid();
        work
    }
    let small = measure(32);
    let large = measure(4096);
    for (operation, (small, large)) in small.into_iter().zip(large).enumerate() {
        assert!(
            large <= small * 4 + 64,
            "cursor operation {operation}: link accesses grew from {small} to {large}"
        );
    }
}
