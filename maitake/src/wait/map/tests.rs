use super::*;
use futures::{future::poll_fn, pin_mut, select_biased, FutureExt};

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc {
    use super::*;
    use crate::loom::sync::Arc;
    use crate::scheduler::Scheduler;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn close() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        let scheduler = Scheduler::new();
        let q: Arc<WaitMap<usize, ()>> = Arc::new(WaitMap::new());

        const TASKS: usize = 10;

        for i in 0..TASKS {
            let wait = q.wait_owned(i);
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
        let q = Arc::new(WaitMap::new());

        const TASKS: usize = 10;

        for i in 0..TASKS {
            let q = q.clone();
            scheduler.spawn(async move {
                let val = q.wait(i).await.unwrap();
                COMPLETED.fetch_add(1, Ordering::SeqCst);

                assert_eq!(val, 100 + i);

                if i < (TASKS - 1) {
                    assert!(matches!(q.wake(&(i + 1), 100 + i + 1), WakeOutcome::Woke));
                } else {
                    assert!(matches!(
                        q.wake(&(i + 1), 100 + i + 1),
                        WakeOutcome::NoMatch(_)
                    ));
                }
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        q.wake(&0, 100);

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }

    #[derive(Debug)]
    struct CountDropKey {
        idx: usize,
        cnt: &'static AtomicUsize,
    }

    impl PartialEq for CountDropKey {
        fn eq(&self, other: &Self) -> bool {
            self.idx.eq(&other.idx)
        }
    }

    impl Drop for CountDropKey {
        fn drop(&mut self) {
            self.cnt.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Debug)]
    struct CountDropVal {
        cnt: &'static AtomicUsize,
    }

    impl Drop for CountDropVal {
        fn drop(&mut self) {
            self.cnt.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn drop_no_wake() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);
        static KEY_DROPS: AtomicUsize = AtomicUsize::new(0);
        static VAL_DROPS: AtomicUsize = AtomicUsize::new(0);

        let scheduler = Scheduler::new();
        let q = Arc::new(WaitMap::<CountDropKey, CountDropVal>::new());

        const TASKS: usize = 10;

        for i in 0..TASKS {
            let q = q.clone();
            scheduler.spawn(async move {
                q.wait(CountDropKey {
                    idx: i,
                    cnt: &KEY_DROPS,
                })
                .await
                .unwrap_err();
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), 0);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        q.close();

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), TASKS);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);
    }

    #[test]
    fn drop_wake_completed() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);
        static KEY_DROPS: AtomicUsize = AtomicUsize::new(0);
        static VAL_DROPS: AtomicUsize = AtomicUsize::new(0);
        static DONT_CARE: AtomicUsize = AtomicUsize::new(0);

        let scheduler = Scheduler::new();
        let q = Arc::new(WaitMap::<CountDropKey, CountDropVal>::new());

        const TASKS: usize = 10;

        for i in 0..TASKS {
            let q = q.clone();
            scheduler.spawn(async move {
                // TODO(AJM): I need to select!() against a one shot channel or something
                // for the "wait hanging" test
                q.wait(CountDropKey {
                    idx: i,
                    cnt: &KEY_DROPS,
                })
                .await
                .unwrap();
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), 0);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        for i in 0..TASKS {
            q.wake(
                &CountDropKey {
                    idx: i,
                    cnt: &DONT_CARE,
                },
                CountDropVal { cnt: &VAL_DROPS },
            );
        }

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), 0);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 0);

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), TASKS);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    }

    #[test]
    fn drop_wake_bailed() {
        crate::util::trace_init();
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);
        static KEY_DROPS: AtomicUsize = AtomicUsize::new(0);
        static VAL_DROPS: AtomicUsize = AtomicUsize::new(0);
        static DONT_CARE: AtomicUsize = AtomicUsize::new(0);
        static BAIL: AtomicBool = AtomicBool::new(false);

        let scheduler = Scheduler::new();
        let q = Arc::new(WaitMap::<CountDropKey, CountDropVal>::new());

        const TASKS: usize = 10;

        for i in 0..TASKS {
            let q = q.clone();
            scheduler.spawn(async move {
                let mut bail_fut = poll_fn(|_| match BAIL.load(Ordering::SeqCst) {
                    false => Poll::Pending,
                    true => Poll::Ready(()),
                })
                .fuse();

                let wait_fut = q
                    .wait(CountDropKey {
                        idx: i,
                        cnt: &KEY_DROPS,
                    })
                    .fuse();
                pin_mut!(wait_fut);

                // NOTE: `select_baised is used specifically to ensure the bail
                // future is processed first.
                select_biased! {
                    _a = bail_fut => {},
                    _b = wait_fut => {
                        COMPLETED.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, 0);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), 0);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 0);
        assert!(!tick.has_remaining);

        for i in 0..TASKS {
            q.wake(
                &CountDropKey {
                    idx: i,
                    cnt: &DONT_CARE,
                },
                CountDropVal { cnt: &VAL_DROPS },
            );
        }

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), 0);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 0);
        BAIL.store(true, Ordering::SeqCst);

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), TASKS);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), TASKS);
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
            let q = Arc::new(WaitMap::<usize, usize>::new());
            let thread = thread::spawn({
                let q = q.clone();
                move || {
                    future::block_on(async {
                        let _val = q.wait(123).await;
                    });
                }
            });

            loom::thread::yield_now();
            let _result = q.wake(&123, 666);
            q.close();

            thread.join().unwrap();
        });
    }

    #[test]
    fn wake_two_sequential() {
        loom::model(|| {
            let q = Arc::new(WaitMap::<usize, usize>::new());
            let q2 = q.clone();

            let thread_a = thread::spawn(move || {
                let wait1 = q2.wait(123);
                let wait2 = q2.wait(456);
                future::block_on(async move {
                    let _ = wait1.await;
                    let _ = wait2.await;
                });
            });

            let thread_b = thread::spawn(move || {
                loom::thread::yield_now();
                let _result = q.wake(&123, 321);
                let _result = q.wake(&456, 654);
                q.close();
            });

            thread_a.join().unwrap();
            thread_b.join().unwrap();
        });
    }

    #[test]
    fn wake_close() {
        use alloc::sync::Arc;

        loom::model(|| {
            let q = Arc::new(WaitMap::<usize, usize>::new());
            let wait1 = q.wait_owned(123);
            let wait2 = q.wait_owned(456);

            let thread1 =
                thread::spawn(move || future::block_on(wait1).expect_err("wait1 must be canceled"));
            let thread2 =
                thread::spawn(move || future::block_on(wait2).expect_err("wait2 must be canceled"));

            q.close();

            thread1.join().unwrap();
            thread2.join().unwrap();
        });
    }
}
