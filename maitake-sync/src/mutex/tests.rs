use mutex_traits::ScopedRawMutex;

use crate::loom::{self, future};
use crate::Mutex;

#[test]
fn basic_single_threaded() {
    loom::model(|| {
        let lock = Mutex::new(1);

        future::block_on(async {
            let mut lock = lock.lock().await;
            assert_eq!(*lock, 1);
            *lock = 2;
        });

        future::block_on(async {
            let mut lock = lock.lock().await;
            assert_eq!(*lock, 2);
            *lock = 3;
        });

        future::block_on(async {
            let mut lock = lock.lock().await;
            assert_eq!(*lock, 3);
            *lock = 4;
        });
    });
}

#[test]
#[cfg(any(loom, feature = "alloc"))]
fn basic_multi_threaded() {
    use crate::loom::{sync::Arc, thread};
    fn incr(lock: &Arc<Mutex<i32>>) -> thread::JoinHandle<()> {
        let lock = lock.clone();
        thread::spawn(move || {
            future::block_on(async move {
                let mut lock = lock.lock().await;
                *lock += 1;
            })
        })
    }

    loom::model(|| {
        let lock = Arc::new(Mutex::new(0));
        let t1 = incr(&lock);
        let t2 = incr(&lock);

        t1.join().unwrap();
        t2.join().unwrap();

        future::block_on(async move {
            let lock = lock.lock().await;
            assert_eq!(*lock, 2)
        })
    });
}

struct NopRawMutex;

unsafe impl ScopedRawMutex for NopRawMutex {
    fn try_with_lock<R>(&self, _: impl FnOnce() -> R) -> Option<R> {
        None
    }

    fn with_lock<R>(&self, _: impl FnOnce() -> R) -> R {
        unimplemented!("this doesn't actually do anything")
    }

    fn is_locked(&self) -> bool {
        true
    }
}

fn assert_future<F: core::future::Future>(_: F) {}

#[test]
fn lock_future_impls_future() {
    loom::model(|| {
        // Mutex with `DefaultMutex` as the `ScopedRawMutex` implementation
        let mutex = Mutex::new(());
        assert_future(mutex.lock());

        // Mutex with a custom `ScopedRawMutex` implementation
        let mutex = Mutex::new_with_raw_mutex((), NopRawMutex);
        assert_future(mutex.lock());
    })
}

#[test]
#[cfg(feature = "alloc")]
fn lock_owned_future_impls_future() {
    loom::model(|| {
        use alloc::sync::Arc;

        // Mutex with `DefaultMutex` as the `ScopedRawMutex` implementation
        let mutex = Arc::new(Mutex::new(()));
        assert_future(mutex.lock_owned());

        // Mutex with a custom `ScopedRawMutex` implementation
        let mutex = Arc::new(Mutex::new_with_raw_mutex((), NopRawMutex));
        assert_future(mutex.lock_owned());
    })
}
