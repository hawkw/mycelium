#![cfg(feature = "alloc")]

mod util;
use maitake::{future, scheduler::Scheduler};

#[test]
fn wakes() {
    util::trace_init();

    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async move {
        future::yield_now().await;
        "hello world!"
    });

    let mut join = tokio_test::task::spawn(join);

    // the join handle should be pending until the scheduler runs.
    tokio_test::assert_pending!(join.poll(), "join handle should be pending");
    assert!(!join.is_woken());

    // tick the scheduler.
    scheduler.tick();

    // the spawned task should complete on this tick.
    assert!(join.is_woken());
    let output = tokio_test::assert_ready_ok!(join.poll(), "join handle should be notified");
    assert_eq!(output, "hello world!");
}
