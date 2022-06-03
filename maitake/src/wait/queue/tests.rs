use super::*;

#[cfg(loom)]
mod loom {
    use super::*;
    use crate::loom::{self, sync::Arc, future, thread};

    #[test]
    fn wake_one() {
        loom::model(|| {
            let q = Arc::new(WaitQueue::new());
            let thread = thread::spawn({
                let q = q.clone();
                move || {
                    future::block_on(async {
                        q.wait().await.expect("queue must not be closed");
                    });
                }
            });

            q.wake();
            thread.join().unwrap();
        });
    }

    #[test]
    fn wake_all_sequential() {
        loom::model(|| {
            let q = Arc::new(WaitQueue::new());
            let wait1 = q.wait();
            let wait2 = q.wait();

            let thread = thread::spawn({
                let q = q.clone();
                move || {
                    q.wake_all();
                }
            });

            future::block_on(async {
                wait1.await.unwrap();
                wait2.await.unwrap();
            });

            thread.join().unwrap();
        });
    }

    #[test]
    fn wake_one_many() {
        loom::model(|| {
            let q = Arc::new(WaitQueue::new());

            fn thread(q: &Arc<WaitQueue>) -> thread::JoinHandle<()> {
                let q = q.clone();
                thread::spawn(move || {
                    future::block_on(async {
                        q.wait().await.expect("queue must not be closed");
                        q.wake();
                    })
                })
            }

            q.wake();

            let thread1 = thread(&q);
            let thread2 = thread(&q);

            thread1.join().unwrap();
            thread2.join().unwrap();

            future::block_on(async {
                q.wait().await.expect("queue must not be closed");
            });
        });
    }
}
