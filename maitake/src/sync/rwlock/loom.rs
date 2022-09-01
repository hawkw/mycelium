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
