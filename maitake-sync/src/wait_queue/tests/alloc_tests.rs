use super::*;
use crate::loom::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
use tokio_test::{assert_pending, assert_ready, assert_ready_ok, task};

const TASKS: usize = 10;

#[test]
fn wake_all() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let q = Arc::new(WaitQueue::new());

    let mut tasks = (0..TASKS)
        .map(|_| {
            let q = q.clone();
            task::spawn(async move {
                q.wait().await.unwrap();
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            })
        })
        .collect::<Vec<_>>();

    for task in &mut tasks {
        assert_pending!(task.poll());
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);

    q.wake_all();

    for task in &mut tasks {
        assert!(task.is_woken());
        assert_ready!(task.poll());
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
}

#[test]
fn close() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let q = Arc::new(WaitQueue::new());

    let mut tasks = (0..TASKS)
        .map(|_| {
            let wait = q.wait_owned();
            task::spawn(async move {
                wait.await.expect_err("dropping the queue must close it");
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            })
        })
        .collect::<Vec<_>>();

    for task in &mut tasks {
        assert_pending!(task.poll());
    }

    q.close();

    for task in &mut tasks {
        assert!(task.is_woken());
        assert_ready!(task.poll());
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
}

#[test]
fn wake_one() {
    let _trace = crate::util::trace_init();
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let q = Arc::new(WaitQueue::new());

    let mut tasks = (0..TASKS)
        .map(|_| {
            let q = q.clone();
            task::spawn(async move {
                q.wait().await.unwrap();
                COMPLETED.fetch_add(1, Ordering::SeqCst);
                q.wake();
            })
        })
        .collect::<Vec<_>>();

    for task in &mut tasks {
        assert_pending!(task.poll());
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), 0);

    q.wake();

    for task in &mut tasks {
        assert!(task.is_woken());
        assert_ready!(task.poll());
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
}

#[test]
fn wake_not_subscribed() {
    let _trace = crate::util::trace_init();

    let q = Arc::new(WaitQueue::new());
    let mut task = task::spawn({
        let q = q.clone();
        async move { q.wait().await.unwrap() }
    });

    assert_pending!(task.poll());

    q.wake();

    assert!(task.is_woken());
    assert_ready!(task.poll());
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
