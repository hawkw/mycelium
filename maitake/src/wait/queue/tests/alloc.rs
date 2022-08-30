
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
