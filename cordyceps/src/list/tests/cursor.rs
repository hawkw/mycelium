use super::*;

/// Based on this test from the standard library's `linked_list::Cursor`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L564-L655
#[test]
fn move_peek() {
    let _ = super::trace_init();

    let one = entry(1);
    let two = entry(2);
    let three = entry(3);
    let four = entry(4);
    let five = entry(5);
    let six = entry(6);

    let mut list = List::new();
    push_all(
        &mut list,
        &[
            six.as_ref(),
            five.as_ref(),
            four.as_ref(),
            three.as_ref(),
            two.as_ref(),
            one.as_ref(),
        ],
    );
    let mut cursor = list.cursor_front_mut();
    assert_eq!(val(cursor.current()), Some(1));
    assert_eq!(val(cursor.peek_next()), Some(2));
    assert_eq!(val(cursor.peek_prev()), None);
    assert_eq!(cursor.index(), Some(0));
    cursor.move_prev();
    assert_eq!(val(cursor.current()), None);
    assert_eq!(val(cursor.peek_next()), Some(1));
    assert_eq!(val(cursor.peek_prev()), Some(6));
    assert_eq!(cursor.index(), None);
    cursor.move_next();
    cursor.move_next();
    assert_eq!(val(cursor.current()), Some(2));
    assert_eq!(val(cursor.peek_next()), Some(3));
    assert_eq!(val(cursor.peek_prev()), Some(1));
    assert_eq!(cursor.index(), Some(1));

    let mut cursor = list.cursor_back_mut();
    assert_eq!(val(cursor.current()), Some(6));
    assert_eq!(val(cursor.peek_next()), None);
    assert_eq!(val(cursor.peek_prev()), Some(5));
    assert_eq!(cursor.index(), Some(5));
    cursor.move_next();
    assert_eq!(val(cursor.current()), None);
    assert_eq!(val(cursor.peek_next()), Some(1));
    assert_eq!(val(cursor.peek_prev()), Some(6));
    assert_eq!(cursor.index(), None);
    cursor.move_prev();
    cursor.move_prev();
    assert_eq!(val(cursor.current()), Some(5));
    assert_eq!(val(cursor.peek_next()), Some(6));
    assert_eq!(val(cursor.peek_prev()), Some(4));
    assert_eq!(cursor.index(), Some(4));

    // let mut m: LinkedList<u32> = LinkedList::new();
    // m.extend(&[1, 2, 3, 4, 5, 6]);
    // let mut cursor = m.cursor_front_mut();
    // assert_eq!(val(cursor.current()), Some(&mut 1));
    // assert_eq!(val(cursor.peek_next()), Some(&mut 2));
    // assert_eq!(val(cursor.peek_prev()), None);
    // assert_eq!(cursor.index(), Some(0));
    // cursor.move_prev();
    // assert_eq!(val(cursor.current()), None);
    // assert_eq!(val(cursor.peek_next()), Some(&mut 1));
    // assert_eq!(val(cursor.peek_prev()), Some(&mut 6));
    // assert_eq!(cursor.index(), None);
    // cursor.move_next();
    // cursor.move_next();
    // assert_eq!(val(cursor.current()), Some(&mut 2));
    // assert_eq!(val(cursor.peek_next()), Some(&mut 3));
    // assert_eq!(val(cursor.peek_prev()), Some(&mut 1));
    // assert_eq!(cursor.index(), Some(1));
    // let mut cursor2 = cursor.as_cursor();
    // assert_eq!(cursor2.current(), Some(&2));
    // assert_eq!(cursor2.index(), Some(1));
    // cursor2.move_next();
    // assert_eq!(cursor2.current(), Some(&3));
    // assert_eq!(cursor2.index(), Some(2));
    // assert_eq!(val(cursor.current()), Some(&mut 2));
    // assert_eq!(cursor.index(), Some(1));

    // let mut m: LinkedList<u32> = LinkedList::new();
    // m.extend(&[1, 2, 3, 4, 5, 6]);
    // let mut cursor = m.cursor_back_mut();
    // assert_eq!(val(cursor.current()), Some(&mut 6));
    // assert_eq!(val(cursor.peek_next()), None);
    // assert_eq!(val(cursor.peek_prev()), Some(&mut 5));
    // assert_eq!(cursor.index(), Some(5));
    // cursor.move_next();
    // assert_eq!(val(cursor.current()), None);
    // assert_eq!(val(cursor.peek_next()), Some(&mut 1));
    // assert_eq!(val(cursor.peek_prev()), Some(&mut 6));
    // assert_eq!(cursor.index(), None);
    // cursor.move_prev();
    // cursor.move_prev();
    // assert_eq!(val(cursor.current()), Some(&mut 5));
    // assert_eq!(val(cursor.peek_next()), Some(&mut 6));
    // assert_eq!(val(cursor.peek_prev()), Some(&mut 4));
    // assert_eq!(cursor.index(), Some(4));
    // let mut cursor2 = cursor.as_cursor();
    // assert_eq!(cursor2.current(), Some(&5));
    // assert_eq!(cursor2.index(), Some(4));
    // cursor2.move_prev();
    // assert_eq!(cursor2.current(), Some(&4));
    // assert_eq!(cursor2.index(), Some(3));
    // assert_eq!(val(cursor.current()), Some(&mut 5));
    // assert_eq!(cursor.index(), Some(4));
}
