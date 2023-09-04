use super::super::*;
use crate::loom::sync::Arc;
use crate::scheduler::Scheduler;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use futures::{future::poll_fn, pin_mut, select_biased, FutureExt};
use tokio_test::task;

#[test]
fn enqueue() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);
    static ENQUEUED: AtomicUsize = AtomicUsize::new(0);

    let scheduler = Scheduler::new();
    let q = Arc::new(WaitMap::new());

    // Create a waiter, but do not tick the scheduler yet
    let q2 = q.clone();
    scheduler.spawn(async move {
        let val = q2.wait(0).await.unwrap();
        COMPLETED.fetch_add(1, Ordering::Relaxed);
        assert_eq!(val, 100);
    });

    // Attempt to wake - but waiter is not enqueued yet
    assert!(matches!(q.wake(&0, 100), WakeOutcome::NoMatch(_)));
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(ENQUEUED.load(Ordering::Relaxed), 0);

    // Create a second waiter - this one that first checks for enqueued
    let q3 = q.clone();
    scheduler.spawn(async move {
        let wait = q3.wait(1);

        pin_mut!(wait);
        wait.as_mut().enqueue().await.unwrap();
        ENQUEUED.fetch_add(1, Ordering::Relaxed);

        let val = wait.await.unwrap();
        COMPLETED.fetch_add(1, Ordering::Relaxed);
        assert_eq!(val, 101);
    });

    // Attempt to wake - but waiter is not enqueued yet
    assert!(matches!(q.wake(&0, 100), WakeOutcome::NoMatch(_)));
    assert!(matches!(q.wake(&1, 101), WakeOutcome::NoMatch(_)));
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(ENQUEUED.load(Ordering::Relaxed), 0);

    // Tick once, we can see the second task moved into the enqueued state
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 0);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(ENQUEUED.load(Ordering::Relaxed), 1);
    assert!(!tick.has_remaining);

    assert!(matches!(q.wake(&0, 100), WakeOutcome::Woke));
    assert!(matches!(q.wake(&1, 101), WakeOutcome::Woke));

    let tick = scheduler.tick();
    assert_eq!(tick.completed, 2);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 2);
    assert_eq!(ENQUEUED.load(Ordering::Relaxed), 1);
    assert!(!tick.has_remaining);
}

#[test]
fn duplicate() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);
    static ENQUEUED: AtomicUsize = AtomicUsize::new(0);
    static ERRORED: AtomicUsize = AtomicUsize::new(0);

    let scheduler = Scheduler::new();
    let q = Arc::new(WaitMap::new());

    // Create a waiter, but do not tick the scheduler yet
    let q2 = q.clone();
    scheduler.spawn(async move {
        let wait = q2.wait(0);

        pin_mut!(wait);
        wait.as_mut().enqueue().await.unwrap();
        ENQUEUED.fetch_add(1, Ordering::Relaxed);

        let val = wait.await.unwrap();
        COMPLETED.fetch_add(1, Ordering::Relaxed);
        assert_eq!(val, 100);
    });

    // Create a waiter, but do not tick the scheduler yet
    let q3 = q.clone();
    scheduler.spawn(async move {
        // Duplicate key!
        let wait = q3.wait(0);

        pin_mut!(wait);
        let result = wait.as_mut().enqueue().await;
        assert!(matches!(result, Err(WaitError::Duplicate)));
        ERRORED.fetch_add(1, Ordering::Relaxed);
    });

    // Tick once, we can see the second task moved into the enqueued state
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 1);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(ENQUEUED.load(Ordering::Relaxed), 1);
    assert_eq!(ERRORED.load(Ordering::Relaxed), 1);
    assert!(!tick.has_remaining);

    assert!(matches!(q.wake(&0, 100), WakeOutcome::Woke));

    let tick = scheduler.tick();
    assert_eq!(tick.completed, 1);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 1);
    assert_eq!(ENQUEUED.load(Ordering::Relaxed), 1);
    assert_eq!(ERRORED.load(Ordering::Relaxed), 1);
    assert!(!tick.has_remaining);
}

#[test]
fn close() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let scheduler = Scheduler::new();
    let q: Arc<WaitMap<usize, ()>> = Arc::new(WaitMap::new());

    const TASKS: usize = 10;

    for i in 0..TASKS {
        let wait = q.wait_owned(i);
        scheduler.spawn(async move {
            wait.await.expect_err("dropping the queue must close it");
            COMPLETED.fetch_add(1, Ordering::Relaxed);
        });
    }

    let tick = scheduler.tick();

    assert_eq!(tick.completed, 0);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert!(!tick.has_remaining);

    q.close();

    let tick = scheduler.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), TASKS);
    assert!(!tick.has_remaining);
}

#[test]
fn wake_one() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let scheduler = Scheduler::new();
    let q = Arc::new(WaitMap::new());

    const TASKS: usize = 10;

    for i in 0..TASKS {
        let q = q.clone();
        scheduler.spawn(async move {
            let val = q.wait(i).await.unwrap();
            COMPLETED.fetch_add(1, Ordering::Relaxed);

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
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert!(!tick.has_remaining);

    q.wake(&0, 100);

    let tick = scheduler.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), TASKS);
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
        self.cnt.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct CountDropVal {
    cnt: &'static AtomicUsize,
}

impl Drop for CountDropVal {
    fn drop(&mut self) {
        self.cnt.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn drop_no_wake() {
    let _trace = crate::util::trace_init();
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
            COMPLETED.fetch_add(1, Ordering::Relaxed);
        });
    }

    let tick = scheduler.tick();

    assert_eq!(tick.completed, 0);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), 0);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), 0);
    assert!(!tick.has_remaining);

    q.close();

    let tick = scheduler.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), TASKS);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), TASKS);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), 0);
    assert!(!tick.has_remaining);
}

#[test]
fn drop_wake_completed() {
    let _trace = crate::util::trace_init();
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
            q.wait(CountDropKey {
                idx: i,
                cnt: &KEY_DROPS,
            })
            .await
            .unwrap();
            COMPLETED.fetch_add(1, Ordering::Relaxed);
        });
    }

    let tick = scheduler.tick();

    assert_eq!(tick.completed, 0);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), 0);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), 0);
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

    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), 0);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), 0);

    let tick = scheduler.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), TASKS);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), TASKS);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), TASKS);
    assert!(!tick.has_remaining);
}

#[test]
fn drop_wake_bailed() {
    let _trace = crate::util::trace_init();
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
            let mut bail_fut = poll_fn(|_| match BAIL.load(Ordering::Relaxed) {
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
                    COMPLETED.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }

    let tick = scheduler.tick();

    assert_eq!(tick.completed, 0);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), 0);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), 0);
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

    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), 0);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), 0);
    BAIL.store(true, Ordering::Relaxed);

    let tick = scheduler.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::Relaxed), 0);
    assert_eq!(KEY_DROPS.load(Ordering::Relaxed), TASKS);
    assert_eq!(VAL_DROPS.load(Ordering::Relaxed), TASKS);
    assert!(!tick.has_remaining);
}
