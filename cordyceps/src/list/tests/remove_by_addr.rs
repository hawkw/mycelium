use super::*;

#[test]
fn first() {
    let _trace = trace_init();
    let a = entry(5);
    let b = entry(7);
    let c = entry(31);

    unsafe {
        // Remove first
        let mut list = [a.as_ref(), b.as_ref(), c.as_ref()]
            .into_iter()
            .collect::<List<_>>();

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
        let mut list = [a.as_ref(), b.as_ref()].into_iter().collect::<List<_>>();

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

        let items = drain_list(&mut list);
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
        let mut list = [a.as_ref(), b.as_ref(), c.as_ref()]
            .into_iter()
            .collect::<List<_>>();

        assert!(list.remove(ptr(&a)).is_some());
        assert_clean!(a);
        list.assert_valid();

        assert_ptr_eq!(b, list.head);
        assert_ptr_eq!(c, b.links.next());
        assert_ptr_eq!(b, c.links.prev());

        let items = drain_list(&mut list);
        assert_eq!([31, 7].to_vec(), items);
        list.assert_valid();
    }

    unsafe {
        let mut list = [a.as_ref(), b.as_ref(), c.as_ref()]
            .into_iter()
            .collect::<List<_>>();

        assert!(list.remove(ptr(&b)).is_some());
        assert_clean!(b);
        list.assert_valid();

        assert_ptr_eq!(c, a.links.next());
        assert_ptr_eq!(a, c.links.prev());

        let items = drain_list(&mut list);
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
        let mut list = [a.as_ref(), b.as_ref(), c.as_ref()]
            .into_iter()
            .collect::<List<_>>();

        assert!(list.remove(ptr(&c)).is_some());
        assert_clean!(c);
        list.assert_valid();

        assert!(b.links.next().is_none());
        assert_ptr_eq!(b, list.tail);

        let items = drain_list(&mut list);
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
        let mut list = [a.as_ref()].into_iter().collect::<List<_>>();

        assert!(list.remove(ptr(&a)).is_some());
        assert_clean!(a);
        list.assert_valid();

        assert!(list.head.is_none());
        assert!(list.tail.is_none());
        let items = drain_list(&mut list);
        assert!(items.is_empty());
    }

    unsafe {
        // Remove last of two
        let mut list = [a.as_ref(), b.as_ref()].into_iter().collect::<List<_>>();

        assert!(list.remove(ptr(&b)).is_some());
        assert_clean!(b);
        list.assert_valid();

        assert_ptr_eq!(a, list.head);
        assert_ptr_eq!(a, list.tail);

        assert!(a.links.next().is_none());
        assert!(a.links.prev().is_none());

        let items = drain_list(&mut list);
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
