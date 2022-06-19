use super::*;

/// Based on this test from the standard library's `linked_list::Cursor`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L564-L655
#[test]
fn move_peek() {
    let _ = super::trace_init();

    let entries = [entry(1), entry(2), entry(3), entry(4), entry(5), entry(6)];
    let mut list = list_from_iter(&entries);

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

/// Based on this test from the standard library's `linked_list::CursorMut`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L657-L714
#[test]
fn cursor_mut_insert() {
    let entries = [entry(1), entry(2), entry(3), entry(4), entry(5), entry(6)];
    let seven = entry(7);
    let eight = entry(8);
    let nine = entry(9);
    let ten = entry(10);

    let mut list = list_from_iter(&entries);

    let mut cursor = list.cursor_front_mut();
    cursor.insert_before(seven.as_ref());
    cursor.insert_after(eight.as_ref());
    assert_eq!(collect_vals(&list), &[7, 1, 8, 2, 3, 4, 5, 6]);

    let mut cursor = list.cursor_front_mut();
    cursor.move_prev();
    cursor.insert_before(nine.as_ref());
    cursor.insert_after(ten.as_ref());
    list.assert_valid();

    assert_eq!(collect_vals(&list), &[10, 7, 1, 8, 2, 3, 4, 5, 6, 9]);

    let mut cursor = list.cursor_front_mut();
    cursor.move_prev();
    assert_eq!(val(cursor.remove_current()), None);
    cursor.move_next();
    cursor.move_next();
    assert_eq!(val(cursor.remove_current()), Some(7));
    cursor.move_prev();
    cursor.move_prev();
    cursor.move_prev();
    assert_eq!(val(cursor.remove_current()), Some(9));
    cursor.move_next();
    assert_eq!(val(cursor.remove_current()), Some(10));
    list.assert_valid();
    assert_eq!(collect_vals(&list), &[1, 8, 2, 3, 4, 5, 6]);

    // let mut cursor = m.cursor_front_mut();
    // let mut p: LinkedList<u32> = LinkedList::new();
    // p.extend(&[100, 101, 102, 103]);
    // let mut q: LinkedList<u32> = LinkedList::new();
    // q.extend(&[200, 201, 202, 203]);
    // cursor.splice_after(p);
    // cursor.splice_before(q);
    // check_links(&m);
    // assert_eq!(
    //     m.iter().cloned().collect::<Vec<_>>(),
    //     &[200, 201, 202, 203, 1, 100, 101, 102, 103, 8, 2, 3, 4, 5, 6]
    // );
    // let mut cursor = m.cursor_front_mut();
    // cursor.move_prev();
    // let tmp = cursor.split_before();
    // assert_eq!(m.into_iter().collect::<Vec<_>>(), &[]);
    // m = tmp;
    // let mut cursor = m.cursor_front_mut();
    // cursor.move_next();
    // cursor.move_next();
    // cursor.move_next();
    // cursor.move_next();
    // cursor.move_next();
    // cursor.move_next();
    // let tmp = cursor.split_after();
    // assert_eq!(
    //     tmp.into_iter().collect::<Vec<_>>(),
    //     &[102, 103, 8, 2, 3, 4, 5, 6]
    // );
    // check_links(&m);
    // assert_eq!(
    //     m.iter().cloned().collect::<Vec<_>>(),
    //     &[200, 201, 202, 203, 1, 100, 101]
    // );
}
