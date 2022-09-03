use super::*;
use crate::loom::{self, future, sync::Arc, thread};

#[test]
fn write() {
    const WRITERS: usize = 2;

    loom::model(|| {
        let lock = Arc::new(RwLock::<usize>::new(0));
        let threads = (0..WRITERS)
            .map(|_| {
                let lock = lock.clone();
                thread::spawn(writer(lock))
            })
            .collect::<Vec<_>>();

        for thread in threads {
            thread.join().expect("writer thread mustn't panic");
        }

        let guard = future::block_on(lock.read());
        assert_eq!(*guard, WRITERS, "final state must equal number of writers");
    });
}

#[test]
fn read_write() {
    const WRITERS: usize = 2;

    loom::model(|| {
        let lock = Arc::new(RwLock::<usize>::new(0));
        let w_threads = (0..WRITERS)
            .map(|_| {
                let lock = lock.clone();
                thread::spawn(writer(lock))
            })
            .collect::<Vec<_>>();

        {
            let guard = future::block_on(lock.read());
            assert!(*guard == 0 || *guard == 1 || *guard == 2);
        }

        for thread in w_threads {
            thread.join().expect("writer thread mustn't panic")
        }

        let guard = future::block_on(lock.read());
        assert_eq!(*guard, WRITERS, "final state must equal number of writers");
    });
}

fn writer(lock: Arc<RwLock<usize>>) -> impl FnOnce() {
    move || {
        future::block_on(async {
            let mut guard = lock.write().await;
            *guard += 1;
        })
    }
}

#[test]
#[cfg(feature = "alloc")]
fn write_owned() {
    const WRITERS: usize = 2;

    loom::model(|| {
        let lock = alloc::sync::Arc::new(RwLock::<usize>::new(0));
        let threads = (0..WRITERS)
            .map(|_| {
                let lock = lock.clone();
                thread::spawn(owned_writer(lock))
            })
            .collect::<Vec<_>>();

        for thread in threads {
            thread.join().expect("writer thread mustn't panic");
        }

        let guard = future::block_on(lock.read());
        assert_eq!(*guard, WRITERS, "final state must equal number of writers");
    });
}

#[test]
#[cfg(feature = "alloc")]
fn read_write_owned() {
    const WRITERS: usize = 2;

    loom::model(|| {
        let lock = alloc::sync::Arc::new(RwLock::<usize>::new(0));
        let w_threads = (0..WRITERS)
            .map(|_| {
                let lock = lock.clone();
                thread::spawn(owned_writer(lock))
            })
            .collect::<Vec<_>>();

        {
            let guard = future::block_on(lock.read_owned());
            assert!(*guard == 0 || *guard == 1 || *guard == 2);
        }

        for thread in w_threads {
            thread.join().expect("writer thread mustn't panic")
        }

        let guard = future::block_on(lock.read_owned());
        assert_eq!(*guard, WRITERS, "final state must equal number of writers");
    });
}

#[cfg(feature = "alloc")]
fn owned_writer(lock: alloc::sync::Arc<RwLock<usize>>) -> impl FnOnce() {
    move || {
        future::block_on(async {
            let mut guard = lock.write_owned().await;
            *guard += 1;
        })
    }
}
