//! Transfer length accounting in the repository's Loom configuration.
#![cfg(loom)]

use cordyceps::{stack::Links, Linked, Stack, TransferStack};
use std::{
    cell::Cell,
    ptr::{self, NonNull},
    rc::Rc,
};

struct Entry {
    links: Links<Self>,
    value: usize,
    drops: Rc<Cell<usize>>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

unsafe impl Linked<Links<Self>> for Entry {
    type Handle = Box<Self>;

    fn into_ptr(handle: Self::Handle) -> NonNull<Self> {
        NonNull::from(Box::leak(handle))
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        Box::from_raw(ptr.as_ptr())
    }

    unsafe fn links(ptr: NonNull<Self>) -> NonNull<Links<Self>> {
        NonNull::new_unchecked(ptr::addr_of_mut!((*ptr.as_ptr()).links))
    }
}

fn entry(value: usize, drops: &Rc<Cell<usize>>) -> Box<Entry> {
    Box::new(Entry {
        links: Links::new(),
        value,
        drops: drops.clone(),
    })
}

fn take_values(stack: &mut Stack<Entry>) -> Vec<usize> {
    let mut values = Vec::new();
    while let Some(entry) = stack.pop() {
        values.push(entry.value);
    }
    values
}

#[test]
fn a_transfer_counts_its_detached_chain_under_loom() {
    loom::model(|| {
        let drops = Rc::new(Cell::new(0));
        let transfer = TransferStack::<Entry>::new();
        assert_eq!(transfer.take_all().len(), 0);

        for value in 0..3 {
            transfer.push(entry(value, &drops));
        }
        let mut first = transfer.take_all();
        assert_eq!(first.len(), 3, "the detached stack must count every node");
        assert!(!first.is_empty());
        assert!(transfer.is_empty());
        first.assert_valid();

        transfer.push(entry(3, &drops));
        transfer.push(entry(4, &drops));
        let mut second = transfer.take_all();
        assert_eq!(second.len(), 2);
        assert!(transfer.is_empty());
        assert_eq!(transfer.take_all().len(), 0);
        second.assert_valid();
        assert_eq!(
            first.len(),
            3,
            "later transfers do not change an earlier one"
        );

        let mut moved = first.take_all();
        assert_eq!(first.len(), 0);
        assert!(first.is_empty());
        assert_eq!(moved.len(), 3);
        assert_eq!(take_values(&mut moved), [2, 1, 0]);
        assert_eq!(moved.len(), 0);
        assert_eq!(take_values(&mut second), [4, 3]);
        assert_eq!(second.len(), 0);
        assert_eq!(drops.get(), 5);

        transfer.push(entry(5, &drops));
        let last = transfer.take_all();
        assert_eq!(last.len(), 1);
        drop(last);
        assert_eq!(drops.get(), 6);
    });
}

mod concurrent_transfer {
    use cordyceps::{stack::Links, Linked, Stack, TransferStack};
    use loom::{sync::Arc, thread};
    use std::{
        ptr::{self, NonNull},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc as DropCounter,
        },
    };

    struct Entry {
        links: Links<Self>,
        value: usize,
        drops: DropCounter<AtomicUsize>,
    }

    impl Drop for Entry {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    unsafe impl Linked<Links<Self>> for Entry {
        type Handle = Box<Self>;

        fn into_ptr(handle: Self::Handle) -> NonNull<Self> {
            NonNull::from(Box::leak(handle))
        }

        unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
            Box::from_raw(ptr.as_ptr())
        }

        unsafe fn links(ptr: NonNull<Self>) -> NonNull<Links<Self>> {
            NonNull::new_unchecked(ptr::addr_of_mut!((*ptr.as_ptr()).links))
        }
    }

    fn entry(value: usize, drops: &DropCounter<AtomicUsize>) -> Box<Entry> {
        Box::new(Entry {
            links: Links::new(),
            value,
            drops: drops.clone(),
        })
    }

    fn checked_values(mut stack: Stack<Entry>) -> Vec<usize> {
        let claimed = stack.len();
        stack.assert_valid();
        let mut values = Vec::new();
        while let Some(entry) = stack.pop() {
            values.push(entry.value);
            assert_eq!(stack.len() + values.len(), claimed);
        }
        assert_eq!(values.len(), claimed);
        assert_eq!(stack.len(), 0);
        assert!(stack.is_empty());
        values
    }

    #[test]
    fn concurrent_transfers_keep_every_detached_chain_counted() {
        loom::model(|| {
            let drops = DropCounter::new(AtomicUsize::new(0));
            let transfer = Arc::new(TransferStack::new());
            transfer.push(entry(0, &drops));

            let producer = thread::spawn({
                let transfer = transfer.clone();
                let drops = drops.clone();
                move || {
                    transfer.push(entry(1, &drops));
                    transfer.push(entry(2, &drops));
                }
            });
            let consumer = thread::spawn({
                let transfer = transfer.clone();
                move || checked_values(transfer.take_all())
            });

            let mut values = checked_values(transfer.take_all());
            values.extend(consumer.join().unwrap());
            producer.join().unwrap();
            values.extend(checked_values(transfer.take_all()));
            values.sort_unstable();
            assert_eq!(values, [0, 1, 2]);
            assert!(transfer.is_empty());
            assert_eq!(transfer.take_all().len(), 0);
            assert_eq!(drops.load(Ordering::Relaxed), 3);
        });
    }
}
