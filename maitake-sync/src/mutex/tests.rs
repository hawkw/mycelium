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
