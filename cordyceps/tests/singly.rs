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

// ---- Stack: what it holds ---------------------------------------------------

#[test]
fn a_stack_counts_what_it_holds() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();

    assert_eq!(stack.len(), 0);
    assert!(stack.is_empty());

    for v in 1..=4 {
        stack.push(entry(&drops, v));
    }
    assert_eq!(stack.len(), 4, "one per push");
    assert!(!stack.is_empty());
    stack.assert_valid();

    drop(stack.pop());
    drop(stack.pop());
    assert_eq!(stack.len(), 2, "one fewer per pop");

    drop(stack.pop());
    drop(stack.pop());
    assert_eq!(stack.len(), 0);
    assert!(stack.is_empty());
    assert!(stack.pop().is_none(), "and it stays empty");
    assert_eq!(stack.len(), 0, "popping an empty stack counts nothing");
    assert_eq!(drops.get(), 4, "every element was dropped exactly once");
}

#[test]
fn taking_every_element_moves_the_count_with_them() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=3 {
        stack.push(entry(&drops, v));
    }

    let mut taken = stack.take_all();

    assert_eq!(stack.len(), 0, "the stack taken from is left empty");
    assert!(stack.is_empty());
    assert_eq!(taken.len(), 3, "and the count went with the elements");
    stack.assert_valid();
    taken.assert_valid();

    // the emptied stack counts from nothing again
    stack.push(entry(&drops, 9));
    assert_eq!(stack.len(), 1, "the stack it was taken from starts over");
    stack.assert_valid();

    // and taking from an empty stack hands back an empty one
    let mut nothing = SortedList::<Entry>::new_min();
    let empty = Stack::<Entry>::new().take_all();
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    // what was taken is an ordinary stack: it removes, drains and counts
    assert_eq!(taken.drain_filter(|e| e.val == 2).count(), 1);
    assert_eq!(taken.len(), 2, "the taken stack keeps its own count");
    taken.assert_valid();
    assert_eq!(drops.get(), 1, "and only the drained one was dropped");

    while let Some(e) = taken.pop() {
        nothing.insert(e);
    }
    assert_eq!(nothing.len(), 2);
    assert_eq!(drain_sorted(&mut nothing), vec![1, 3]);
    assert_eq!(drain_stack(&mut stack), vec![9]);
}

#[test]
fn a_transfer_stack_hands_over_a_counted_stack() {
    let drops = counter();
    let transfer = TransferStack::<Entry>::new();
    for v in 1..=3 {
        transfer.push(entry(&drops, v));
    }

    let mut taken = transfer.take_all();
    assert_eq!(
        taken.len(),
        3,
        "a stack taken from a TransferStack knows its size"
    );
    taken.assert_valid();

    let empty = transfer.take_all();
    assert_eq!(
        empty.len(),
        0,
        "and taking again yields an empty, counted stack"
    );
    assert!(empty.is_empty());
    assert!(transfer.is_empty());

    // what came over is an ordinary stack, with a count that keeps up
    assert_eq!(val(taken.front()), Some(3));
    assert_eq!(taken.drain_filter(|e| e.val == 1).count(), 1);
    assert_eq!(taken.len(), 2);
    taken.assert_valid();

    // a second round, so the count cannot be a one-off
    for v in 4..=8 {
        transfer.push(entry(&drops, v));
    }
    let mut second = transfer.take_all();
    assert_eq!(second.len(), 5, "each hand-over counts what it took");
    second.assert_valid();
    assert!(transfer.take_all().is_empty());

    assert_eq!(drain_stack(&mut taken), vec![3, 2]);
    assert_eq!(drain_stack(&mut second).len(), 5);
    assert_eq!(drops.get(), 8, "every element was dropped, and none twice");

    // A `TransferStack` is pushed to through a shared reference, so the count
    // has to come out right when the elements arrive from several threads at
    // once and are taken while they are still arriving.
    use std::sync::Arc;
    use std::thread;

    const PRODUCERS: usize = 8;
    const EACH: usize = 250;

    let shared: Arc<TransferStack<Counted>> = Arc::new(TransferStack::new());
    let mut producers = Vec::new();
    for p in 0..PRODUCERS {
        let shared = Arc::clone(&shared);
        producers.push(thread::spawn(move || {
            for i in 0..EACH {
                shared.push(counted((p * EACH + i) as i32));
            }
        }));
    }

    // Take repeatedly while they push, then once more for whatever landed
    // after the last round, which is how the crate's own tests drain one.
    let mut seen: Vec<i32> = Vec::new();
    loop {
        let mut batch = shared.take_all();
        batch.assert_valid();
        let claimed = batch.len();
        let mut carried = 0usize;
        while let Some(e) = batch.pop() {
            seen.push(e.val);
            carried += 1;
        }
        assert_eq!(
            claimed, carried,
            "a hand-over said it held {claimed} elements and carried {carried}"
        );
        if producers.iter().all(thread::JoinHandle::is_finished) {
            break;
        }
        thread::yield_now();
    }
    for p in producers {
        p.join().expect("a producer panicked");
    }
    let mut last = shared.take_all();
    last.assert_valid();
    let claimed = last.len();
    let mut carried = 0usize;
    while let Some(e) = last.pop() {
        seen.push(e.val);
        carried += 1;
    }
    assert_eq!(claimed, carried, "the last hand-over miscounted");
    assert!(shared.is_empty(), "and the transfer stack is left empty");
    assert_eq!(shared.take_all().len(), 0);

    seen.sort();
    assert_eq!(
        seen,
        (0..(PRODUCERS * EACH) as i32).collect::<Vec<_>>(),
        "every element pushed came back exactly once"
    );
}

#[test]
fn the_front_of_a_stack_is_borrowed_not_taken() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    assert!(stack.front().is_none(), "an empty stack has no front");

    stack.push(entry(&drops, 1));
    stack.push(entry(&drops, 2));

    assert_eq!(val(stack.front()), Some(2), "the element pop would return");
    assert_eq!(val(stack.front()), Some(2), "and it is still there");
    assert_eq!(stack.len(), 2, "borrowing it took nothing out");
    assert_eq!(drops.get(), 0, "and dropped nothing");
    assert_eq!(stack.pop().map(|e| e.val), Some(2), "pop still gets it");

    // it keeps pointing at whatever the front becomes, however it got there
    assert_eq!(val(stack.front()), Some(1), "and now at what is left");
    stack.push(entry(&drops, 3));
    assert_eq!(val(stack.front()), Some(3));

    let mut taken = stack.take_all();
    assert!(
        stack.front().is_none(),
        "a stack with nothing in it has no front"
    );
    assert_eq!(
        val(taken.front()),
        Some(3),
        "and the one taken from it does"
    );

    assert_eq!(taken.drain_filter(|e| e.val == 3).count(), 1);
    assert_eq!(
        val(taken.front()),
        Some(1),
        "draining the front moves it on"
    );

    taken.clear();
    assert!(taken.front().is_none(), "and clearing leaves none");
    assert_eq!(taken.len(), 0);
}

#[test]
fn clearing_a_stack_drops_everything_it_held() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=5 {
        stack.push(entry(&drops, v));
    }

    stack.clear();

    assert_eq!(drops.get(), 5, "every element was dropped, not leaked");
    assert_eq!(stack.len(), 0);
    assert!(stack.is_empty());
    stack.assert_valid();

    stack.push(entry(&drops, 9));
    assert_eq!(stack.len(), 1, "and the stack is usable afterwards");
    assert_eq!(val(stack.front()), Some(9));
}

// ---- Stack: taking one element back out -------------------------------------

#[test]
fn a_stack_gives_back_the_element_it_is_asked_for() {
    let drops = counter();
    for target in [1, 2, 3] {
        let mut stack = Stack::<Entry>::new();
        let mut wanted = None;
        // pushed 1, 2, 3, so the chain runs 3 -> 2 -> 1: this covers removing
        // the head, the middle, and the last element.
        for v in 1..=3 {
            let e = entry(&drops, v);
            if v == target {
                wanted = Some(addr(&e));
            }
            stack.push(e);
        }

        let removed = unsafe { stack.remove(wanted.unwrap()) };
        assert_eq!(
            removed.map(|e| e.val),
            Some(target),
            "removing {target} hands back that element"
        );
        assert_eq!(stack.len(), 2, "and the stack is one shorter");
        stack.assert_valid();

        let rest: Vec<i32> = drain_stack(&mut stack);
        let expected: Vec<i32> = [3, 2, 1].into_iter().filter(|&v| v != target).collect();
        assert_eq!(rest, expected, "the others keep their order");
    }
}

#[test]
fn a_stack_does_not_recognise_an_element_it_never_held() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=3 {
        stack.push(entry(&drops, v));
    }

    // an element which was never pushed anywhere
    let stranger = entry(&drops, 99);
    let stranger_addr = addr(&stranger);
    assert!(
        unsafe { stack.remove(stranger_addr) }.is_none(),
        "an element this stack never held is not found"
    );
    assert_eq!(stack.len(), 3, "and nothing was taken out");
    stack.assert_valid();

    // an element which belongs to a different stack
    let mut other = Stack::<Entry>::new();
    let e = entry(&drops, 4);
    let elsewhere = addr(&e);
    other.push(e);
    assert!(
        unsafe { stack.remove(elsewhere) }.is_none(),
        "nor is one belonging to another stack"
    );
    assert_eq!(stack.len(), 3);
    assert_eq!(other.len(), 1, "and the other stack is untouched");
    stack.assert_valid();
    other.assert_valid();

    assert_eq!(drain_stack(&mut stack), vec![3, 2, 1]);
    drop(stranger);
}

#[test]
fn draining_a_stack_keeps_what_the_predicate_refused() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=6 {
        stack.push(entry(&drops, v));
    }

    let taken: Vec<i32> = stack
        .drain_filter(|e| e.val % 2 == 0)
        .map(|e| e.val)
        .collect();

    assert_eq!(taken, vec![6, 4, 2], "the accepted ones, from the front");
    assert_eq!(stack.len(), 3, "the length counts only what is left");
    stack.assert_valid();
    assert_eq!(
        drops.get(),
        3,
        "the yielded elements were dropped by the caller, and no others"
    );
    assert_eq!(
        drain_stack(&mut stack),
        vec![5, 3, 1],
        "the rest keep order"
    );
}

#[test]
fn abandoning_a_drain_leaves_the_rest_of_the_stack_alone() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in 1..=5 {
        stack.push(entry(&drops, v));
    }

    {
        let mut drain = stack.drain_filter(|_| true);
        assert_eq!(drain.next().map(|e| e.val), Some(5));
        assert_eq!(drain.next().map(|e| e.val), Some(4));
        // dropped without being exhausted
    }

    assert_eq!(stack.len(), 3, "only what was actually taken is gone");
    stack.assert_valid();
    assert_eq!(drops.get(), 2, "and only those two were dropped");
    assert_eq!(drain_stack(&mut stack), vec![3, 2, 1]);
}

// ---- SortedList: what it holds ----------------------------------------------

#[test]
fn a_sorted_list_counts_what_it_holds() {
    let drops = counter();
    let mut list = SortedList::<Entry>::new_min();
    assert_eq!(list.len(), 0);
    assert!(list.is_empty());

    for v in [5, 1, 4, 2] {
        list.insert(entry(&drops, v));
    }
    assert_eq!(list.len(), 4, "one per insert, wherever it landed");
    assert!(!list.is_empty());
    list.assert_valid();

    assert_eq!(list.pop_front().map(|e| e.val), Some(1));
    assert_eq!(list.len(), 3, "one fewer per pop");
    list.assert_valid();

    assert_eq!(drain_sorted(&mut list), vec![2, 4, 5]);
    assert_eq!(list.len(), 0);
    assert!(list.pop_front().is_none());
    assert_eq!(list.len(), 0, "popping an empty list counts nothing");
}

#[test]
fn the_front_of_a_sorted_list_is_the_one_pop_would_take() {
    let drops = counter();
    let mut list = SortedList::<Entry>::new_min();
    assert!(list.front().is_none(), "an empty list has no front");

    for v in [7, 3, 9] {
        list.insert(entry(&drops, v));
    }

    assert_eq!(val(list.front()), Some(3), "the smallest, under new_min");
    assert_eq!(list.len(), 3, "borrowing it took nothing out");
    assert_eq!(drops.get(), 0);
    assert_eq!(list.pop_front().map(|e| e.val), Some(3));

    let mut max = SortedList::<Entry>::new_max();
    for v in [7, 3, 9] {
        max.insert(entry(&drops, v));
    }
    assert_eq!(val(max.front()), Some(9), "the largest, under new_max");
}

#[test]
fn a_sorted_list_gives_back_the_element_it_is_asked_for() {
    let drops = counter();
    for target in [1, 2, 3] {
        let mut list = SortedList::<Entry>::new_min();
        let mut wanted = None;
        for v in [2, 3, 1] {
            let e = entry(&drops, v);
            if v == target {
                wanted = Some(addr(&e));
            }
            list.insert(e);
        }

        let removed = unsafe { list.remove(wanted.unwrap()) };
        assert_eq!(
            removed.map(|e| e.val),
            Some(target),
            "removing {target} hands back that element"
        );
        assert_eq!(list.len(), 2);
        list.assert_valid();

        let expected: Vec<i32> = [1, 2, 3].into_iter().filter(|&v| v != target).collect();
        assert_eq!(drain_sorted(&mut list), expected, "the rest stay in order");
    }

    // an element this list never held
    let mut list = sorted_of(&drops, &[1, 2]);
    let stranger = entry(&drops, 99);
    assert!(
        unsafe { list.remove(addr(&stranger)) }.is_none(),
        "an element this list never held is not found"
    );
    assert_eq!(list.len(), 2, "and nothing was taken out");
    list.assert_valid();
    drop(stranger);
}

// ---- SortedList: merging -----------------------------------------------------

#[test]
fn merging_two_lists_leaves_one_sorted_list_and_one_empty() {
    let drops = counter();
    let mut left = sorted_of(&drops, &[1, 4, 6]);
    let mut right = sorted_of(&drops, &[2, 3, 7]);

    left.merge(&mut right);

    assert_eq!(left.len(), 6, "the receiver holds both lists' elements");
    assert_eq!(right.len(), 0, "and the other is left empty");
    assert!(right.is_empty());
    left.assert_valid();
    right.assert_valid();
    assert_eq!(drain_sorted(&mut left), vec![1, 2, 3, 4, 6, 7]);

    // merging an empty list in either direction changes nothing but the count
    let mut held = sorted_of(&drops, &[5, 8]);
    let mut nothing = SortedList::<Entry>::new_min();
    held.merge(&mut nothing);
    assert_eq!(held.len(), 2);
    assert_eq!(drain_sorted(&mut held), vec![5, 8]);

    let mut empty = SortedList::<Entry>::new_min();
    let mut full = sorted_of(&drops, &[5, 8]);
    empty.merge(&mut full);
    assert_eq!(
        empty.len(),
        2,
        "merging into an empty list takes everything"
    );
    assert_eq!(full.len(), 0);
    assert_eq!(drain_sorted(&mut empty), vec![5, 8]);
}

#[test]
fn a_merge_sorts_by_the_receiving_lists_own_ordering() {
    let drops = counter();
    // the receiver is largest-first; the argument is smallest-first.
    let mut receiver = SortedList::<Entry>::new_max();
    for v in [6, 2] {
        receiver.insert(entry(&drops, v));
    }
    let mut argument = sorted_of(&drops, &[1, 9]);

    receiver.merge(&mut argument);

    assert_eq!(receiver.len(), 4);
    receiver.assert_valid();
    assert_eq!(
        drain_sorted(&mut receiver),
        vec![9, 6, 2, 1],
        "the result is ordered the way the receiver orders, not the argument"
    );
}

#[test]
fn a_merge_keeps_the_receivers_element_first_when_they_are_equal() {
    let drops = counter();
    // `Entry` compares on `val` alone, so these two are equal but tell apart by
    // which list they came from.
    let mut receiver = SortedList::<Entry>::new_min();
    let mine = entry(&drops, 5);
    let mine_addr = addr(&mine);
    receiver.insert(entry(&drops, 1));
    receiver.insert(mine);

    let mut argument = SortedList::<Entry>::new_min();
    let theirs = entry(&drops, 5);
    let theirs_addr = addr(&theirs);
    argument.insert(theirs);
    argument.insert(entry(&drops, 9));

    receiver.merge(&mut argument);
    receiver.assert_valid();

    // walk to the first element which compares equal to 5 and check which one
    // it is, by identity rather than by value.
    assert_eq!(receiver.pop_front().map(|e| e.val), Some(1));
    let first_five = receiver.pop_front().expect("two fives are held");
    assert_eq!(first_five.val, 5);
    assert_eq!(
        NonNull::from(&*first_five),
        mine_addr,
        "of two equal elements the receiver's comes first"
    );
    let second_five = receiver.pop_front().expect("the argument's five");
    assert_eq!(NonNull::from(&*second_five), theirs_addr);
    assert_eq!(receiver.pop_front().map(|e| e.val), Some(9));
}

// ---- SortedList: splitting ---------------------------------------------------

#[test]
fn splitting_a_list_divides_both_its_elements_and_its_length() {
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3, 4, 5]);

    let mut tail = list.split_off(2);

    assert_eq!(list.len(), 2, "the first `at` elements stay");
    assert_eq!(tail.len(), 3, "and the rest are handed back");
    list.assert_valid();
    tail.assert_valid();
    assert_eq!(val(list.front()), Some(1));
    assert_eq!(
        val(tail.front()),
        Some(3),
        "the tail begins where the head ended"
    );

    // both halves are ordinary lists afterwards: what goes in lands in order
    list.insert(entry(&drops, 0));
    tail.insert(entry(&drops, 4));
    list.assert_valid();
    tail.assert_valid();
    assert_eq!(list.len(), 3);
    assert_eq!(tail.len(), 4);
    assert_eq!(drops.get(), 0, "splitting dropped nothing");
    assert_eq!(drain_sorted(&mut list), vec![0, 1, 2]);
    assert_eq!(drain_sorted(&mut tail), vec![3, 4, 4, 5]);

    // and every position of a longer list divides it the same way
    for at in 0..=6 {
        let drops = counter();
        let mut whole = sorted_of(&drops, &[1, 2, 3, 4, 5, 6]);
        let mut rest = whole.split_off(at);
        whole.assert_valid();
        rest.assert_valid();
        assert_eq!(whole.len(), at, "splitting at {at} left the wrong number");
        assert_eq!(rest.len(), 6 - at);
        assert_eq!(drops.get(), 0, "splitting at {at} dropped something");
        let mut seen = drain_sorted(&mut whole);
        seen.extend(drain_sorted(&mut rest));
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 6], "splitting at {at} lost order");
    }
}

#[test]
fn splitting_at_either_end_moves_everything_or_nothing() {
    let drops = counter();

    let mut list = sorted_of(&drops, &[1, 2, 3]);
    let mut all = list.split_off(0);
    assert_eq!(list.len(), 0, "splitting at zero leaves nothing behind");
    assert!(list.is_empty());
    assert_eq!(all.len(), 3);
    list.assert_valid();
    all.assert_valid();
    assert_eq!(drain_sorted(&mut all), vec![1, 2, 3]);

    let mut list = sorted_of(&drops, &[1, 2, 3]);
    let mut none = list.split_off(3);
    assert_eq!(none.len(), 0, "splitting at the length hands back nothing");
    assert!(none.is_empty());
    assert_eq!(list.len(), 3, "and takes nothing away");
    list.assert_valid();
    none.assert_valid();
    assert!(none.pop_front().is_none());
    assert_eq!(drain_sorted(&mut list), vec![1, 2, 3]);

    // and on an empty list, zero is the only legal split
    let mut empty = SortedList::<Entry>::new_min();
    let nothing = empty.split_off(0);
    assert_eq!(nothing.len(), 0);
    assert_eq!(empty.len(), 0);
}

#[test]
fn splitting_past_the_end_is_refused() {
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3]);

    assert!(
        panicked(|| {
            let mut list = sorted_of(&counter(), &[1, 2, 3]);
            let _ = list.split_off(4);
        }),
        "a position past the end is a caller error"
    );

    assert!(
        list.try_split_off(4).is_none(),
        "and the non-panicking form answers None"
    );
    assert_eq!(list.len(), 3, "having taken nothing");
    list.assert_valid();

    let mut at_end = list.try_split_off(3).expect("the length itself is legal");
    assert_eq!(at_end.len(), 0);
    assert_eq!(drain_sorted(&mut list), vec![1, 2, 3]);
    assert!(at_end.pop_front().is_none());
}

#[test]
fn a_split_off_list_still_sorts_the_way_its_parent_did() {
    let drops = counter();
    let mut parent = SortedList::<Entry>::new_max();
    for v in [1, 5, 9] {
        parent.insert(entry(&drops, v));
    }

    let mut tail = parent.split_off(1);
    // the parent is largest-first, so the tail holds 5 then 1
    tail.insert(entry(&drops, 7));
    tail.assert_valid();

    assert_eq!(
        drain_sorted(&mut tail),
        vec![7, 5, 1],
        "the split list orders new elements the way its parent does"
    );
}

// ---- SortedList: draining ----------------------------------------------------

#[test]
fn draining_a_sorted_list_leaves_the_rest_in_order() {
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3, 4, 5, 6]);

    let taken: Vec<i32> = list
        .drain_filter(|e| e.val % 2 == 0)
        .map(|e| e.val)
        .collect();

    assert_eq!(taken, vec![2, 4, 6], "the accepted ones, in sorted order");
    assert_eq!(list.len(), 3);
    list.assert_valid();
    assert_eq!(drops.get(), 3, "and only those were dropped");
    assert_eq!(
        drain_sorted(&mut list),
        vec![1, 3, 5],
        "the rest stay sorted"
    );

    // a drain which accepts nothing leaves the list exactly as it was
    let mut untouched = sorted_of(&drops, &[1, 2, 3]);
    assert_eq!(untouched.drain_filter(|_| false).count(), 0);
    assert_eq!(untouched.len(), 3);
    untouched.assert_valid();
    assert_eq!(drain_sorted(&mut untouched), vec![1, 2, 3]);
}

#[test]
fn abandoning_a_drain_leaves_the_rest_of_the_list_alone() {
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 2, 3, 4, 5]);

    {
        let mut drain = list.drain_filter(|_| true);
        assert_eq!(drain.next().map(|e| e.val), Some(1));
        assert_eq!(drain.next().map(|e| e.val), Some(2));
    }

    assert_eq!(list.len(), 3, "only what was actually taken is gone");
    list.assert_valid();
    assert_eq!(drops.get(), 2);
    assert_eq!(drain_sorted(&mut list), vec![3, 4, 5]);
}

#[test]
fn an_element_taken_out_of_one_list_can_be_put_into_another() {
    let drops = counter();
    let mut from = Stack::<Entry>::new();
    let mut wanted = None;
    for v in 1..=3 {
        let e = entry(&drops, v);
        if v == 2 {
            wanted = Some(addr(&e));
        }
        from.push(e);
    }

    let taken = unsafe { from.remove(wanted.unwrap()) }.expect("it is in this stack");
    assert_eq!(taken.val, 2);

    // the element must come back carrying no link into the stack it left,
    // otherwise it drags that stack's tail along with it.
    let mut into = Stack::<Entry>::new();
    into.push(taken);
    into.assert_valid();
    assert_eq!(
        into.len(),
        1,
        "the new stack holds exactly what was put in it"
    );
    assert_eq!(drain_stack(&mut into), vec![2]);

    from.assert_valid();
    assert_eq!(from.len(), 2, "and the stack it left is unharmed");
    assert_eq!(drain_stack(&mut from), vec![3, 1]);

    // the same for a sorted list, where a stale link would also break the order
    let drops = counter();
    let mut list = sorted_of(&drops, &[1, 5, 9]);
    let five = NonNull::from(
        list.iter()
            .find(|e| e.val == 5)
            .expect("the list holds a five"),
    );
    let taken = unsafe { list.remove(five) }.expect("it is in this list");
    let mut other = SortedList::<Entry>::new_min();
    other.insert(entry(&drops, 4));
    other.insert(taken);
    other.assert_valid();
    assert_eq!(other.len(), 2);
    assert_eq!(drain_sorted(&mut other), vec![4, 5]);
    list.assert_valid();
    assert_eq!(drain_sorted(&mut list), vec![1, 9]);

    // and the same for an element a drain hands over: it is the caller's, so it
    // must arrive carrying no link into the list it was taken from.
    let drops = counter();
    let mut from = sorted_of(&drops, &[1, 2, 3, 4]);
    let taken: Vec<Box<Entry>> = from.drain_filter(|e| e.val % 2 == 0).collect();
    assert_eq!(taken.len(), 2);
    from.assert_valid();

    let mut into = SortedList::<Entry>::new_min();
    for e in taken {
        into.insert(e);
    }
    into.assert_valid();
    assert_eq!(into.len(), 2, "the new list holds exactly what was drained");
    assert_eq!(drain_sorted(&mut into), vec![2, 4]);
    assert_eq!(
        drain_sorted(&mut from),
        vec![1, 3],
        "and the old one is unharmed"
    );

    // and for a stack's drain, where a stale link would drag the tail across
    let drops = counter();
    let mut from = Stack::<Entry>::new();
    for v in 1..=4 {
        from.push(entry(&drops, v));
    }
    // the chain runs 4 -> 3 -> 2 -> 1, so the drain hands over 4 then 2
    let taken: Vec<Box<Entry>> = from.drain_filter(|e| e.val % 2 == 0).collect();
    assert_eq!(taken.len(), 2);
    from.assert_valid();

    let mut into = Stack::<Entry>::new();
    for e in taken {
        into.push(e);
    }
    into.assert_valid();
    assert_eq!(
        into.len(),
        2,
        "the new stack holds exactly what was drained"
    );
    assert_eq!(drain_stack(&mut into), vec![2, 4]);
    assert_eq!(
        drain_stack(&mut from),
        vec![3, 1],
        "and the stack it left is unharmed"
    );
    assert_eq!(drops.get(), 4, "and nothing was lost along the way");
}

#[test]
fn clearing_a_sorted_list_drops_everything_it_held() {
    let drops = counter();
    let mut list = sorted_of(&drops, &[3, 1, 4, 1, 5]);
    assert_eq!(list.len(), 5);

    list.clear();

    assert_eq!(drops.get(), 5, "every element was dropped, not leaked");
    assert_eq!(list.len(), 0);
    assert!(list.is_empty());
    assert!(list.front().is_none());
    list.assert_valid();

    list.insert(entry(&drops, 2));
    assert_eq!(list.len(), 1, "and the list is usable afterwards");
    assert_eq!(val(list.front()), Some(2));
    assert_eq!(drain_sorted(&mut list), vec![2]);

    // clearing an already-empty list is harmless
    let mut empty = SortedList::<Entry>::new_min();
    empty.clear();
    assert_eq!(empty.len(), 0);
    empty.assert_valid();
}

#[test]
fn a_list_built_out_of_a_stack_counts_what_it_took() {
    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    for v in [4, 1, 3] {
        stack.push(entry(&drops, v));
    }
    assert_eq!(stack.len(), 3);

    let mut list = SortedList::from_stack_min(stack);
    assert_eq!(list.len(), 3, "the list counts everything the stack held");
    list.assert_valid();
    assert_eq!(val(list.front()), Some(1));
    assert_eq!(drain_sorted(&mut list), vec![1, 3, 4]);

    // largest-first, and from an empty stack
    let mut stack = Stack::<Entry>::new();
    for v in [4, 1, 3] {
        stack.push(entry(&drops, v));
    }
    let mut list = SortedList::from_stack_max(stack);
    assert_eq!(list.len(), 3);
    list.assert_valid();
    assert_eq!(drain_sorted(&mut list), vec![4, 3, 1]);

    let empty = SortedList::from_stack_min(Stack::<Entry>::new());
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    // extending an existing list counts what it is given
    let mut list = sorted_of(&drops, &[2]);
    list.extend((0..3).map(|v| entry(&drops, v)));
    assert_eq!(list.len(), 4, "one per element extended in");
    list.assert_valid();
    assert_eq!(drain_sorted(&mut list), vec![0, 1, 2, 2]);
}

#[test]
fn asking_for_the_length_does_not_walk_the_list() {
    let mut stack = Stack::<Counted>::new();
    let mut list = SortedList::<Counted>::new_min();
    for v in 0..8 {
        stack.push(counted(v));
        list.insert(counted(v));
    }

    // Reading the length is a constant-time answer, so it cannot be reaching
    // for anyone's links; a length that is counted by walking would.
    let before = link_reads();
    for _ in 0..4 {
        assert_eq!(stack.len(), 8);
        assert_eq!(list.len(), 8);
        assert!(!stack.is_empty());
        assert!(!list.is_empty());
    }
    assert_eq!(
        link_reads(),
        before,
        "reading the length reached for the elements' links, so it is not answering in constant time"
    );
}

#[test]
fn the_debug_check_walks_the_whole_list() {
    let mut stack = Stack::<Counted>::new();
    let mut list = SortedList::<Counted>::new_min();
    for v in 0..6 {
        stack.push(counted(v));
        list.insert(counted(v));
    }

    // `assert_valid` confirms the collection holds together, which it cannot do
    // without visiting what is linked into it.
    let before = link_reads();
    stack.assert_valid();
    let after_stack = link_reads();
    assert!(
        after_stack - before >= 6,
        "the stack's debug check visited {} elements of 6",
        after_stack - before
    );

    list.assert_valid();
    assert!(
        link_reads() - after_stack >= 6,
        "the list's debug check visited {} elements of 6",
        link_reads() - after_stack
    );
}

#[test]
fn a_panicking_comparator_leaves_both_lists_accounted_for() {
    // The ordering function is the caller's, and this one refuses to look at 99.
    fn refuses_99(a: &Entry, b: &Entry) -> Ordering {
        assert!(
            a.val != 99 && b.val != 99,
            "this ordering function does not handle 99"
        );
        a.val.cmp(&b.val)
    }

    let drops = counter();
    let mut receiver = SortedList::<Entry>::new_with_cmp(refuses_99);
    receiver.insert(entry(&drops, 1));

    // 100 sorts after 99, so the merge still has an element in hand when the
    // ordering function gives up on the one before it.
    let mut other = SortedList::<Entry>::new_min();
    other.insert(entry(&drops, 2));
    other.insert(entry(&drops, 99));
    other.insert(entry(&drops, 100));

    assert!(
        panicked(|| {
            receiver.merge(&mut other);
        }),
        "the ordering function refuses 99, so the merge cannot finish"
    );

    // Whatever it managed before giving up, every element is still accounted
    // for: linked into one of the two lists, or dropped. None may be stranded,
    // and neither list may claim more than it holds.
    receiver.assert_valid();
    other.assert_valid();
    assert_eq!(
        drops.get() + receiver.len() + other.len(),
        4,
        "an element went missing when the ordering function gave up"
    );

    // and both lists are still usable
    receiver.insert(entry(&drops, 0));
    receiver.assert_valid();
    assert_eq!(val(receiver.front()), Some(0));
}

#[test]
fn an_insert_the_ordering_function_refuses_loses_nothing() {
    // The same rule where nothing is being moved between lists: the element was
    // handed over, so it is the caller's no longer, and the list either holds
    // it or has dropped it.
    fn refuses_99(a: &Entry, b: &Entry) -> Ordering {
        assert!(
            a.val != 99 && b.val != 99,
            "this ordering function does not handle 99"
        );
        a.val.cmp(&b.val)
    }

    let drops = counter();
    let mut list = SortedList::<Entry>::new_with_cmp(refuses_99);
    list.insert(entry(&drops, 1));
    list.insert(entry(&drops, 3));

    assert!(
        panicked(|| {
            list.insert(entry(&drops, 99));
        }),
        "the ordering function refuses 99, so the insert cannot finish"
    );

    list.assert_valid();
    assert_eq!(list.len(), 2, "the list did not count what it never took");
    assert_eq!(
        drops.get(),
        1,
        "and the element it could not place was dropped"
    );

    // still usable, and the refused element left nothing of itself behind
    list.insert(entry(&drops, 2));
    list.assert_valid();
    assert_eq!(list.len(), 3);
    assert_eq!(drain_sorted(&mut list), vec![1, 2, 3]);
}

#[test]
fn removing_asks_for_one_element_not_one_that_merely_looks_like_it() {
    let drops = counter();

    // two different elements which compare equal, held by two different lists
    let mut mine = sorted_of(&drops, &[1, 5, 9]);
    let mut theirs = SortedList::<Entry>::new_min();
    let twin = entry(&drops, 5);
    let twin_addr = addr(&twin);
    theirs.insert(twin);

    assert!(
        unsafe { mine.remove(twin_addr) }.is_none(),
        "the element asked for belongs to another list, however much it looks like one of ours"
    );
    assert_eq!(mine.len(), 3, "so nothing came out of this list");
    assert_eq!(theirs.len(), 1, "nor out of the other");
    mine.assert_valid();
    theirs.assert_valid();
    assert_eq!(
        drain_sorted(&mut mine),
        vec![1, 5, 9],
        "and this list still holds its own five"
    );
    assert_eq!(drain_sorted(&mut theirs), vec![5]);

    // a pointer to something this list has already handed back
    let mut list = sorted_of(&drops, &[2, 4]);
    let four = NonNull::from(list.iter().find(|e| e.val == 4).expect("a four is held"));
    let taken = unsafe { list.remove(four) }.expect("it is in the list");
    assert!(
        unsafe { list.remove(four) }.is_none(),
        "asking again for something already taken out finds nothing"
    );
    assert_eq!(list.len(), 1, "and takes nothing else in its place");
    list.assert_valid();
    assert_eq!(drain_sorted(&mut list), vec![2]);
    drop(taken);

    // and the same of a stack
    let mut stack = Stack::<Entry>::new();
    let e = entry(&drops, 7);
    let seven = addr(&e);
    stack.push(e);
    stack.push(entry(&drops, 8));
    let taken = unsafe { stack.remove(seven) }.expect("it is in the stack");
    assert!(
        unsafe { stack.remove(seven) }.is_none(),
        "nor does a stack hand the same element over twice"
    );
    assert_eq!(stack.len(), 1);
    stack.assert_valid();
    drop(taken);
}

// ---- ownership ---------------------------------------------------------------

#[test]
fn dropping_a_collection_drops_everything_still_linked() {
    let drops = counter();

    {
        let mut stack = Stack::<Entry>::new();
        for v in 1..=3 {
            stack.push(entry(&drops, v));
        }
        drop(stack.pop());
        assert_eq!(drops.get(), 1);
    }
    assert_eq!(drops.get(), 3, "the stack dropped the two it still held");

    let drops = counter();
    {
        let mut list = sorted_of(&drops, &[1, 2, 3, 4]);
        let removed = unsafe { list.remove(NonNull::from(list.front().unwrap().get_ref())) };
        assert_eq!(removed.map(|e| e.val), Some(1));
        assert_eq!(drops.get(), 1);
    }
    assert_eq!(drops.get(), 4, "the list dropped the three it still held");
}

#[test]
fn the_collections_stay_consistent_across_a_run_of_operations() {
    // A fixed sequence of operations, checked after every one against a plain
    // `Vec` standing in for each collection. Values are distinct, so the model
    // is exact and there is nothing to guess about where equal elements land.
    let mut seed: u64 = 0x2545F4914F6CDD1D;
    let mut roll = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };

    let drops = counter();
    let mut stack = Stack::<Entry>::new();
    let mut list = SortedList::<Entry>::new_min();
    // index 0 is the front of each, which is what `front`, `pop` and
    // `pop_front` all speak about.
    let mut stack_model: Vec<i32> = Vec::new();
    let mut list_model: Vec<i32> = Vec::new();
    // elements handed back out, kept alive so their addresses stay valid for
    // `remove` to answer `None` about.
    let mut escrow: Vec<Box<Entry>> = Vec::new();
    // one element of the stack whose address is known to still be linked
    let mut tracked: Option<NonNull<Entry>> = None;
    let mut made = 0usize;

    for step in 0..400 {
        match roll(12) {
            0 => {
                let v = made as i32;
                made += 1;
                let e = entry(&drops, v);
                let at = addr(&e);
                stack.push(e);
                stack_model.insert(0, v);
                if tracked.is_none() {
                    tracked = Some(at);
                }
            }
            1 => {
                let got = stack.pop().map(|e| e.val);
                let want = if stack_model.is_empty() {
                    None
                } else {
                    Some(stack_model.remove(0))
                };
                assert_eq!(got, want, "step {step}: the stack popped the wrong element");
                tracked = None;
            }
            2 => {
                let v = made as i32;
                made += 1;
                list.insert(entry(&drops, v));
                let at = list_model.partition_point(|&x| x <= v);
                list_model.insert(at, v);
            }
            3 => {
                let got = list.pop_front().map(|e| e.val);
                let want = if list_model.is_empty() {
                    None
                } else {
                    Some(list_model.remove(0))
                };
                assert_eq!(got, want, "step {step}: the list popped the wrong element");
            }
            4 => {
                assert_eq!(
                    val(stack.front()),
                    stack_model.first().copied(),
                    "step {step}: the wrong element is at the front of the stack"
                );
                assert_eq!(
                    val(list.front()),
                    list_model.first().copied(),
                    "step {step}: the wrong element is at the front of the list"
                );
            }
            5 => {
                if roll(2) == 0 {
                    stack.clear();
                    stack_model.clear();
                    tracked = None;
                } else {
                    list.clear();
                    list_model.clear();
                }
            }
            6 => {
                // take back an element the stack really holds
                if let Some(ptr) = tracked.take() {
                    let got = unsafe { stack.remove(ptr) }.expect("it is in this stack");
                    let at = stack_model
                        .iter()
                        .position(|&x| x == got.val)
                        .expect("the model holds it too");
                    stack_model.remove(at);
                    escrow.push(got);
                }
            }
            7 => {
                // and ask both of them for one neither holds
                if let Some(e) = escrow.first() {
                    let ptr = addr(e);
                    assert!(
                        unsafe { stack.remove(ptr) }.is_none(),
                        "step {step}: the stack claimed an element it does not hold"
                    );
                    assert!(
                        unsafe { list.remove(ptr) }.is_none(),
                        "step {step}: the list claimed an element it does not hold"
                    );
                }
            }
            8 => {
                let m = 1 + roll(4) as i32;
                let taken: Vec<i32> = stack
                    .drain_filter(|e| e.val % m == 0)
                    .map(|e| e.val)
                    .collect();
                let want: Vec<i32> = stack_model.iter().copied().filter(|v| v % m == 0).collect();
                assert_eq!(
                    taken, want,
                    "step {step}: the stack drained the wrong elements"
                );
                stack_model.retain(|v| v % m != 0);
                tracked = None;
            }
            9 => {
                let m = 1 + roll(4) as i32;
                let taken: Vec<i32> = list
                    .drain_filter(|e| e.val % m == 0)
                    .map(|e| e.val)
                    .collect();
                let want: Vec<i32> = list_model.iter().copied().filter(|v| v % m == 0).collect();
                assert_eq!(
                    taken, want,
                    "step {step}: the list drained the wrong elements"
                );
                list_model.retain(|v| v % m != 0);
            }
            10 => {
                // a second list, built the other way round, merged in
                let mut other = SortedList::<Entry>::new_max();
                for _ in 0..roll(4) {
                    let v = made as i32;
                    made += 1;
                    other.insert(entry(&drops, v));
                    let at = list_model.partition_point(|&x| x <= v);
                    list_model.insert(at, v);
                }
                list.merge(&mut other);
                assert_eq!(
                    other.len(),
                    0,
                    "step {step}: the merged list was left holding something"
                );
                assert!(other.is_empty());
            }
            11 => {
                let at = roll(list.len() as u64 + 1) as usize;
                let mut tail = list.split_off(at);
                assert_eq!(
                    list.len(),
                    at,
                    "step {step}: the wrong number stayed behind"
                );
                assert_eq!(tail.len(), list_model.len() - at);
                tail.assert_valid();
                // inserting into the half that moved must still land in order
                let v = made as i32;
                made += 1;
                tail.insert(entry(&drops, v));
                let at2 = list_model.partition_point(|&x| x <= v);
                list_model.insert(at2.max(at), v);
                tail.assert_valid();
                list.merge(&mut tail);
            }
            _ => {
                let n = list.len();
                assert!(
                    list.try_split_off(n + 1).is_none(),
                    "step {step}: splitting past the end was allowed"
                );
                let mut taken = stack.take_all();
                assert_eq!(taken.len(), stack_model.len());
                assert_eq!(stack.len(), 0);
                assert!(stack.is_empty());
                taken.assert_valid();
                while let Some(e) = taken.pop() {
                    stack.push(e);
                }
                stack_model.reverse();
                tracked = None;
            }
        }

        stack.assert_valid();
        list.assert_valid();
        assert_eq!(
            stack.len(),
            stack_model.len(),
            "step {step}: the stack miscounted"
        );
        assert_eq!(
            list.len(),
            list_model.len(),
            "step {step}: the list miscounted"
        );
        assert_eq!(stack.is_empty(), stack_model.is_empty());
        assert_eq!(list.is_empty(), list_model.is_empty());
        assert_eq!(
            drops.get() + stack.len() + list.len() + escrow.len(),
            made,
            "step {step}: an element is neither linked, held, nor dropped"
        );
    }

    assert_eq!(drain_stack(&mut stack), stack_model);
    assert_eq!(drain_sorted(&mut list), list_model);
    let held = escrow.len();
    drop(escrow);
    assert_eq!(
        drops.get(),
        made,
        "everything made was accounted for at the end"
    );
    assert!(held <= made);
}

thread_local! {
    static COMPARISONS_LEFT: Cell<usize> = const { Cell::new(usize::MAX) };
}

fn interrupts_after_comparisons(a: &Entry, b: &Entry) -> Ordering {
    COMPARISONS_LEFT.with(|remaining| {
        let left = remaining.get();
        assert_ne!(left, 0, "the ordering function stopped");
        remaining.set(left - 1);
    });
    a.val.cmp(&b.val)
}

#[test]
fn an_insert_interrupted_further_along_loses_nothing() {
    let mut interrupted = false;
    for allowance in 0..16 {
        let drops = counter();
        COMPARISONS_LEFT.with(|remaining| remaining.set(usize::MAX));
        let mut list = SortedList::<Entry>::new_with_cmp(interrupts_after_comparisons);
        for value in [1, 3, 7, 9] {
            list.insert(entry(&drops, value));
        }

        COMPARISONS_LEFT.with(|remaining| remaining.set(allowance));
        let did_panic = panicked(|| list.insert(entry(&drops, 5)));
        COMPARISONS_LEFT.with(|remaining| remaining.set(usize::MAX));
        interrupted |= did_panic;

        list.assert_valid();
        assert_eq!(list.len() + drops.get(), 5, "allowance {allowance}");
        let claimed = list.len();
        let values = drain_sorted(&mut list);
        assert_eq!(claimed, values.len());
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
        if !did_panic {
            assert_eq!(values, [1, 3, 5, 7, 9]);
        }
        assert_eq!(drops.get(), 5);
        list.insert(entry(&drops, 4));
        assert_eq!(list.len(), 1);
        assert_eq!(drain_sorted(&mut list), [4]);
        assert_eq!(drops.get(), 6);
    }
    assert!(interrupted, "the caller's comparator panic must propagate");
}

#[test]
fn a_merge_interrupted_further_along_loses_nothing() {
    let mut interrupted = false;
    for allowance in 0..40 {
        let drops = counter();
        COMPARISONS_LEFT.with(|remaining| remaining.set(usize::MAX));
        let mut receiver = SortedList::<Entry>::new_with_cmp(interrupts_after_comparisons);
        for value in [1, 4, 8, 12] {
            receiver.insert(entry(&drops, value));
        }
        let mut other = SortedList::<Entry>::new_max();
        for value in [2, 3, 6, 11, 13] {
            other.insert(entry(&drops, value));
        }

        COMPARISONS_LEFT.with(|remaining| remaining.set(allowance));
        let did_panic = panicked(|| receiver.merge(&mut other));
        COMPARISONS_LEFT.with(|remaining| remaining.set(usize::MAX));
        interrupted |= did_panic;

        receiver.assert_valid();
        other.assert_valid();
        assert_eq!(
            receiver.len() + other.len() + drops.get(),
            9,
            "allowance {allowance}"
        );
        let receiver_len = receiver.len();
        let other_len = other.len();
        let receiver_values = drain_sorted(&mut receiver);
        let other_values = drain_sorted(&mut other);
        assert_eq!(receiver_len, receiver_values.len());
        assert_eq!(other_len, other_values.len());
        assert!(receiver_values.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(other_values.windows(2).all(|pair| pair[0] > pair[1]));
        if !did_panic {
            assert!(other_values.is_empty());
            assert_eq!(receiver_values, [1, 2, 3, 4, 6, 8, 11, 12, 13]);
        }
        assert_eq!(drops.get(), 9);
        receiver.insert(entry(&drops, 0));
        other.insert(entry(&drops, 10));
        assert_eq!(receiver.len(), 1);
        assert_eq!(other.len(), 1);
        assert_eq!(drain_sorted(&mut receiver), [0]);
        assert_eq!(drain_sorted(&mut other), [10]);
        assert_eq!(drops.get(), 11);
    }
    assert!(interrupted, "the caller's comparator panic must propagate");
}

#[test]
fn predicate_unwind_preserves_ownership_and_counts() {
    for fail_at in 0..6 {
        let drops = counter();
        let mut stack = Stack::<Entry>::new();
        for value in 0..6 {
            stack.push(entry(&drops, value));
        }
        let mut seen = 0;
        let mut yielded = Vec::new();
        assert!(panicked(|| {
            let drain = stack.drain_filter(|e| {
                assert_ne!(seen, fail_at, "predicate failure");
                seen += 1;
                e.val % 2 == 0
            });
            for value in drain {
                yielded.push(value);
            }
        }));
        stack.assert_valid();
        assert_eq!(stack.len() + yielded.len() + drops.get(), 6);
        let claimed = stack.len();
        let remaining = drain_stack(&mut stack);
        assert_eq!(claimed, remaining.len());
        assert!(remaining.windows(2).all(|pair| pair[0] > pair[1]));
        let mut other = SortedList::<Entry>::new_min();
        other.extend(yielded);
        other.assert_valid();
        drop(other);
        assert_eq!(drops.get(), 6);

        let drops = counter();
        let mut list = sorted_of(&drops, &[0, 1, 2, 3, 4, 5]);
        let mut seen = 0;
        let mut yielded = Vec::new();
        assert!(panicked(|| {
            let drain = list.drain_filter(|e| {
                assert_ne!(seen, fail_at, "predicate failure");
                seen += 1;
                e.val % 2 == 0
            });
            for value in drain {
                yielded.push(value);
            }
        }));
        list.assert_valid();
        assert_eq!(list.len() + yielded.len() + drops.get(), 6);
        let claimed = list.len();
        let remaining = drain_sorted(&mut list);
        assert_eq!(claimed, remaining.len());
        assert!(remaining.windows(2).all(|pair| pair[0] < pair[1]));
        let mut other = Stack::<Entry>::new();
        for value in yielded {
            other.push(value);
        }
        other.assert_valid();
        drop(other);
        assert_eq!(drops.get(), 6);
    }
}
