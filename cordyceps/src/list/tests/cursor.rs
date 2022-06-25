use super::*;

/// Based on this test from the standard library's `linked_list::Cursor`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L564-L655
#[test]
fn move_peek_front() {
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
}

/// Based on this test from the standard library's `linked_list::Cursor`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L564-L655
#[test]
fn move_peek_back() {
    let _ = super::trace_init();

    let entries = [entry(1), entry(2), entry(3), entry(4), entry(5), entry(6)];
    let mut list = list_from_iter(&entries);

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
}

/// Based on this test from the standard library's `linked_list::Cursor`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L564-L655
#[test]
fn as_cursor_move_peek_front() {
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
    let mut cursor2 = cursor.as_cursor();
    assert_eq!(val(cursor2.current()), Some(2));
    assert_eq!(cursor2.index(), Some(1));
    cursor2.move_next();
    assert_eq!(val(cursor2.current()), Some(3));
    assert_eq!(cursor2.index(), Some(2));
    assert_eq!(val(cursor.current()), Some(2));
    assert_eq!(cursor.index(), Some(1));
}

/// Based on this test from the standard library's `linked_list::Cursor`
/// interface:
/// https://github.com/rust-lang/rust/blob/ec21d7ea3ca8e96863f175fbd4a6bfee79529d6c/library/alloc/src/collections/linked_list/tests.rs#L564-L655
#[test]
fn as_cursor_move_peek_back() {
    let _ = super::trace_init();

    let entries = [entry(1), entry(2), entry(3), entry(4), entry(5), entry(6)];
    let mut list = list_from_iter(&entries);
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
    let mut cursor2 = cursor.as_cursor();
    assert_eq!(val(cursor2.current()), Some(5));
    assert_eq!(cursor2.index(), Some(4));
    cursor2.move_prev();
    assert_eq!(val(cursor2.current()), Some(4));
    assert_eq!(cursor2.index(), Some(3));
    assert_eq!(val(cursor.current()), Some(5));
    assert_eq!(cursor.index(), Some(4));
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
}

#[test]
fn cursor_mut_splice() {
    let entries_a = [entry(1), entry(2), entry(3), entry(4), entry(5), entry(6)];
    let entries_b = [entry(100), entry(101), entry(102), entry(103)];
    let entries_c = [entry(200), entry(201), entry(202), entry(203)];

    let mut a = list_from_iter(&entries_a);
    let b = list_from_iter(&entries_b);
    let c = list_from_iter(&entries_c);

    let mut cursor = a.cursor_front_mut();
    cursor.splice_after(b);
    cursor.splice_before(c);

    assert_valid!(a);
    assert_eq!(
        collect_vals(&a),
        &[200, 201, 202, 203, 1, 100, 101, 102, 103, 2, 3, 4, 5, 6]
    );
    let mut cursor = a.cursor_front_mut();
    cursor.move_prev();
    let tmp = cursor.split_before();
    assert_eq!(collect_vals(&a), &[]);
    a = tmp;
    let mut cursor = a.cursor_front_mut();
    cursor.move_next();
    cursor.move_next();
    cursor.move_next();
    cursor.move_next();
    cursor.move_next();
    cursor.move_next();

    let tmp = cursor.split_after();
    assert_eq!(collect_vals(&tmp), &[102, 103, 2, 3, 4, 5, 6]);
    assert_eq!(collect_vals(&a), &[200, 201, 202, 203, 1, 100, 101]);
    assert_valid!(tmp);
    assert_valid!(a);
}

#[test]
fn cursor_mut_split_after() {
    let _trace = trace_init();
    let entries = [entry(1), entry(2), entry(3), entry(4), entry(5)];
    let vals = [1, 2, 3, 4, 5];

    // test all splits
    for i in 0..vals.len() + 1 {
        let _span = tracing::info_span!("split_after", i).entered();

        let mut list = list_from_iter(&entries);
        let mut cursor = list.cursor_front_mut();

        for _ in 0..i {
            cursor.move_next();
        }

        tracing::info!(?cursor);

        // split off at this index
        let split = cursor.split_after();
        tracing::info!(?split, ?list);

        assert_valid!(list);
        assert_valid!(split);

        let split_entries = split.iter().map(|entry| entry.val).collect::<Vec<_>>();
        let list_entries = list.iter().map(|entry| entry.val).collect::<Vec<_>>();
        tracing::info!(?list_entries, ?split_entries);

        if i < vals.len() {
            assert_eq!(list_entries, vals[..i + 1], "list after split");
            assert_eq!(split_entries, vals[i + 1..], "split");
        } else {
            // if we were at the end of the list, the split should contain
            // everything.
            assert_eq!(list_entries, &[], "list after split");
            assert_eq!(split_entries, vals[..], "split");
        }
    }
}

#[test]
fn cursor_mut_split_before() {
    let _trace = trace_init();
    let entries = [entry(1), entry(2), entry(3), entry(4), entry(5)];
    let vals = [1, 2, 3, 4, 5];

    // test all splits
    for i in 0..vals.len() + 1 {
        let _span = tracing::info_span!("split_before", i).entered();
        let mut list = list_from_iter(&entries);
        let mut cursor = list.cursor_front_mut();
        for _ in 0..i {
            cursor.move_next();
        }

        tracing::info!(?cursor);

        // split off at this index
        let split = cursor.split_before();
        tracing::info!(?split, ?list);

        assert_valid!(list);
        assert_valid!(split);

        let split_entries = split.iter().map(|entry| entry.val).collect::<Vec<_>>();
        let list_entries = list.iter().map(|entry| entry.val).collect::<Vec<_>>();
        tracing::info!(?list_entries, ?split_entries);

        if i > 0 {
            assert_eq!(list_entries, vals[i..], "list after split");
            assert_eq!(split_entries, vals[..i], "split");
        } else {
            // if we were at the beginning of the list, the split should contain
            // everything.
            assert_eq!(list_entries, vals[..], "list after split");
            assert_eq!(split_entries, &[], "split");
        }
    }
}
