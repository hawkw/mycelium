use super::*;
use crate::loom::sync::Arc;
use crate::scheduler::Scheduler;
use core::sync::atomic::{AtomicUsize, Ordering};
use tokio_test::{assert_pending, assert_ready, assert_ready_ok, task};

#[test]
fn wake_all() {
    let _trace = crate::util::trace_init();
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
    let _trace = crate::util::trace_init();
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
    let _trace = crate::util::trace_init();
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

#[test]
fn wake_not_subscribed() {
    let _trace = crate::util::trace_init();

    let scheduler = Scheduler::new();
    let q = Arc::new(WaitQueue::new());
    let task = scheduler.spawn({
        let q = q.clone();
        async move { q.wait().await.unwrap() }
    });

    q.wake();

    let tick = scheduler.tick();
    assert_eq!(tick.completed, 1);
    assert!(task.is_complete());
}

#[test]
fn wake_after_subscribe() {
    let _trace = crate::util::trace_init();
    let q = WaitQueue::new();
    let mut future = task::spawn(q.wait());

    future.enter(|_, f| assert_pending!(f.subscribe()));

    q.wake();
    assert_ready_ok!(future.poll());
    future.enter(|_, f| assert_ready_ok!(f.subscribe()));
}

#[test]
fn poll_after_subscribe() {
    let _trace = crate::util::trace_init();
    let q = WaitQueue::new();
    let mut future = task::spawn(q.wait());

    future.enter(|_, f| assert_pending!(f.subscribe()));
    assert_pending!(future.poll());
}

#[test]
fn subscribe_after_poll() {
    let _trace = crate::util::trace_init();

    let q = WaitQueue::new();
    let mut future = task::spawn(q.wait());

    assert_pending!(future.poll());
    future.enter(|_, f| assert_pending!(f.subscribe()));
}

#[test]
fn subscribe_consumes_wakeup() {
    let _trace = crate::util::trace_init();
    let q = WaitQueue::new();

    // Add a wakeup.
    q.wake();

    let mut future1 = task::spawn(q.wait());
    future1.enter(|_, f| assert_ready_ok!(f.subscribe()));

    let mut future2 = task::spawn(q.wait());
    future2.enter(|_, f| assert_pending!(f.subscribe()));
}
