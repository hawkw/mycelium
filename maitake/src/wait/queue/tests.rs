#[cfg(any(loom, feature = "alloc"))]
use super::*;

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc {
    use super::*;
    use crate::loom::sync::Arc;
    use crate::scheduler::Scheduler;
    use core::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn wake_all() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let scheduler = Scheduler::new();
        let q = Arc::new(WaitQueue::new());

        const TASKS: usize = 10;

        for _ in 0..TASKS {
            let q = q.clone();
            scheduler.spawn(async move {
                q.wait().await.unwrap();
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        q.wake_all();

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }

    #[test]
    fn close() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let scheduler = Scheduler::new();
        let q = Arc::new(WaitQueue::new());

        const TASKS: usize = 10;

        for _ in 0..TASKS {
            let wait = q.wait_owned();
            scheduler.spawn(async move {
                wait.await.expect_err("dropping the queue must close it");
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        q.close();

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }

    #[test]
    fn wake_one() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let scheduler = Scheduler::new();
        let q = Arc::new(WaitQueue::new());

        const TASKS: usize = 10;

        for _ in 0..TASKS {
            let q = q.clone();
            scheduler.spawn(async move {
                q.wait().await.unwrap();
                COMPLETED.fetch_add(1, Ordering::SeqCst);
                q.wake();
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        q.wake();

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }
}

#[cfg(loom)]
mod loom {
    use super::*;
    use crate::loom::{self, future, sync::Arc, thread};

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
    fn wake_all_concurrent() {
        use alloc::sync::Arc;

        loom::model(|| {
            let q = Arc::new(WaitQueue::new());
            let wait1 = q.wait_owned();
            let wait2 = q.wait_owned();

            let thread1 =
                thread::spawn(move || future::block_on(wait1).expect("wait1 must not fail"));
            let thread2 =
                thread::spawn(move || future::block_on(wait2).expect("wait2 must not fail"));

            q.wake_all();

            thread1.join().unwrap();
            thread2.join().unwrap();
        });
    }

    #[test]
    fn wake_close() {
        use alloc::sync::Arc;

        loom::model(|| {
            let q = Arc::new(WaitQueue::new());
            let wait1 = q.wait_owned();
            let wait2 = q.wait_owned();

            let thread1 =
                thread::spawn(move || future::block_on(wait1).expect_err("wait1 must be canceled"));
            let thread2 =
                thread::spawn(move || future::block_on(wait2).expect_err("wait2 must be canceled"));

            q.close();

            thread1.join().unwrap();
            thread2.join().unwrap();
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

    #[test]
    fn wake_mixed() {
        loom::model(|| {
            let q = Arc::new(WaitQueue::new());

            let thread1 = thread::spawn({
                let q = q.clone();
                move || {
                    q.wake_all();
                }
            });

            let thread2 = thread::spawn({
                let q = q.clone();
                move || {
                    q.wake();
                }
            });

            let thread3 = thread::spawn(move || {
                future::block_on(q.wait()).unwrap();
            });

            thread1.join().unwrap();
            thread2.join().unwrap();
            thread3.join().unwrap();
        });
    }

    #[test]
    fn drop_wait_future() {
        use futures_util::future::poll_fn;
        use std::future::Future;
        use std::task::Poll;

        loom::model(|| {
            let q = Arc::new(WaitQueue::new());

            let thread1 = thread::spawn({
                let q = q.clone();
                move || {
                    let mut wait = Box::pin(q.wait());

                    future::block_on(poll_fn(|cx| {
                        if wait.as_mut().poll(cx).is_ready() {
                            q.wake();
                        }
                        Poll::Ready(())
                    }));
                }
            });

            let thread2 = thread::spawn({
                let q = q.clone();
                move || {
                    future::block_on(async {
                        q.wait().await.unwrap();
                        // Trigger second notification
                        q.wake();
                        q.wait().await.unwrap();
                    });
                }
            });

            q.wake();

            thread1.join().unwrap();
            thread2.join().unwrap();
        });
    }
}
