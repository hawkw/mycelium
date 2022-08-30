use super::*;
use crate::scheduler::Scheduler;
use core::sync::atomic::{AtomicBool, Ordering};

#[test]
fn wakes() {
    crate::util::trace_init();

    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async move {
        future::yield_now().await;
        "hello world!"
    });

    let mut join = tokio_test::task::spawn(join);

    // the join handle should be pending until the scheduler runs.
    tokio_test::assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
    assert!(!join.is_woken());

    // tick the scheduler.
    scheduler.tick();

    // the spawned task should complete on this tick.
    assert!(join.is_woken());
    let output =
        tokio_test::assert_ready_ok!(test_dbg!(join.poll()), "join handle should be notified");
    assert_eq!(test_dbg!(output), "hello world!");
}

#[test]
fn drop_join_handle() {
    crate::util::trace_init();
    static COMPLETED: AtomicBool = AtomicBool::new(false);
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async move {
        future::yield_now().await;
        COMPLETED.store(true, Ordering::Relaxed);
    });

    drop(join);

    // tick the scheduler.
    scheduler.tick();

    assert!(COMPLETED.load(Ordering::Relaxed))
}
