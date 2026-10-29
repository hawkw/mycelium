#![cfg(feature = "alloc")]

use maitake::{
    future,
    scheduler::Scheduler,
    task::{BoxStorage, Task, TaskPhase},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

/// A future that never completes and never wakes itself.
struct Park;

impl Future for Park {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        Poll::Pending
    }
}

#[test]
fn a_spawned_task_has_a_pending_wakeup() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    let status = join.status().expect("the handle still references the task");

    assert!(status.is_woken(), "a freshly spawned task has a wakeup");
    assert!(!status.is_polling());
    assert!(!status.is_completed());
    assert!(!status.is_canceled());
    assert!(status.has_join_handle());
    assert!(!status.has_output());
}

#[test]
fn beginning_a_poll_consumes_the_wakeup() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    assert!(join.status().unwrap().is_woken());

    scheduler.tick();

    let status = join.status().unwrap();
    assert!(
        !status.is_woken(),
        "polling the task consumes its pending wakeup"
    );
    assert!(!status.is_polling(), "the poll has already finished");
    assert!(!status.is_completed());
}

#[test]
fn a_task_observes_itself_being_polled() {
    let scheduler = Scheduler::new();
    let observed = Arc::new(Mutex::new(None));

    let inner = scheduler.clone();
    let recorder = observed.clone();
    let join = scheduler.spawn(async move {
        let me = inner.current_task().expect("a task is being polled");
        *recorder.lock().unwrap() = Some(me.status());
    });

    assert!(!join.status().unwrap().is_polling());
    scheduler.tick();

    let inside = observed.lock().unwrap().expect("the task ran");
    assert!(inside.is_polling(), "a task is polling while it runs");
    assert!(!inside.is_completed(), "it has not returned Ready yet");
    assert!(
        !join.status().unwrap().is_polling(),
        "the task is no longer polling once the tick is over"
    );
}

#[test]
fn a_completed_task_holds_its_output_until_it_is_taken() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async { 42usize });
    let task = join.task_ref();

    scheduler.tick();
    let status = task.status();
    assert!(status.is_completed());
    assert!(status.has_output(), "the output is waiting to be taken");

    let mut handle = tokio_test::task::spawn(join);
    assert_eq!(tokio_test::assert_ready_ok!(handle.poll()), 42);

    let status = task.status();
    assert!(status.is_completed(), "the task is still complete");
    assert!(!status.has_output(), "the output has been taken");
}

#[test]
fn a_join_handle_that_has_yielded_its_output_has_no_status() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async { 7usize });
    assert!(join.status().is_some());

    scheduler.tick();
    let mut handle = tokio_test::task::spawn(join);
    assert_eq!(tokio_test::assert_ready_ok!(handle.poll()), 7);

    assert!(
        handle.into_inner().status().is_none(),
        "the handle no longer references a task"
    );
}

#[test]
fn dropping_the_join_handle_is_visible_in_the_status() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    let task = join.task_ref();

    assert!(task.status().has_join_handle());
    drop(join);
    assert!(
        !task.status().has_join_handle(),
        "the task no longer has a join handle"
    );
}

#[test]
fn cancelling_a_task_does_not_complete_it() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async {
        future::yield_now().await;
    });
    let task = join.task_ref();

    assert!(!task.status().is_canceled());
    assert!(task.cancel(), "the task should cancel");

    let status = task.status();
    assert!(status.is_canceled(), "cancellation is visible immediately");
    assert!(
        !status.is_completed(),
        "cancelling does not complete a task"
    );

    scheduler.tick();

    let status = task.status();
    assert!(status.is_canceled());
    assert!(
        !status.is_completed(),
        "a canceled task is not polled again, so it never completes"
    );
}

#[test]
fn cloning_a_task_ref_changes_the_reference_count() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    let task = join.task_ref();

    let before = task.status().ref_count();
    let clone = task.clone();
    assert_eq!(
        task.status().ref_count(),
        before + 1,
        "cloning a TaskRef adds a reference"
    );

    drop(clone);
    assert_eq!(
        task.status().ref_count(),
        before,
        "dropping the clone removes it again"
    );
    assert!(before > 0, "a referenced task has live references");
}

#[test]
fn a_task_reports_its_own_status() {
    let task = Task::<Scheduler, _, BoxStorage>::new(Park);
    let status = task.status();

    assert!(!status.is_polling());
    assert!(!status.is_completed());
    assert!(!status.is_canceled());
    assert!(!status.has_output());
    assert!(!status.has_join_handle(), "it has not been spawned");
    assert_eq!(
        status.ref_count(),
        1,
        "a task that has just been created has one reference"
    );
}

#[test]
fn task_status_is_a_copyable_comparable_value() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async { 1usize });
    let task = join.task_ref();

    let before = task.status();
    let copied = before;
    assert_eq!(before, copied, "a status compares equal to its own copy");
    assert_eq!(before, before.clone());

    scheduler.tick();
    let after = task.status();
    assert_ne!(
        before, after,
        "a status is a snapshot, so it does not follow the task"
    );
    assert!(!before.is_completed() && after.is_completed());
    let _ = format!("{before:?}");
}

#[test]
fn phase_is_scheduled_before_the_first_poll() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);

    let status = join.status().unwrap();
    assert_eq!(status.phase(), TaskPhase::Scheduled);
    assert!(!status.is_finished());
}

#[test]
fn phase_is_running_while_the_task_is_polled() {
    let scheduler = Scheduler::new();
    let observed = Arc::new(Mutex::new(None));

    let inner = scheduler.clone();
    let recorder = observed.clone();
    scheduler.spawn(async move {
        let me = inner.current_task().expect("a task is being polled");
        *recorder.lock().unwrap() = Some(me.status());
    });
    scheduler.tick();

    let inside = observed.lock().unwrap().expect("the task ran");
    assert_eq!(inside.phase(), TaskPhase::Running);
    assert!(!inside.is_finished(), "it is still going");
}

#[test]
fn phase_is_idle_once_the_wakeup_is_consumed() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    scheduler.tick();

    let status = join.status().unwrap();
    assert_eq!(
        status.phase(),
        TaskPhase::Idle,
        "the task was polled, returned pending, and nothing woke it"
    );
    assert!(!status.is_finished());
}

#[test]
fn phase_is_complete_when_the_future_returns() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async { 3usize });
    let task = join.task_ref();
    scheduler.tick();

    let status = task.status();
    assert_eq!(status.phase(), TaskPhase::Complete);
    assert!(status.is_finished());
}

#[test]
fn phase_is_canceled_when_the_task_is_canceled_first() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    let task = join.task_ref();
    assert!(task.cancel());

    let status = task.status();
    assert_eq!(status.phase(), TaskPhase::Canceled);
    assert!(status.is_finished(), "a canceled task is not polled again");

    scheduler.tick();
    assert_eq!(
        task.status().phase(),
        TaskPhase::Canceled,
        "ticking does not move a canceled task on"
    );
}

#[test]
fn completing_outranks_a_later_cancellation() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(async { 11usize });
    let task = join.task_ref();
    scheduler.tick();
    task.cancel();

    let status = task.status();
    assert!(status.is_completed());
    assert!(status.is_canceled(), "both facts hold at once");
    assert_eq!(
        status.phase(),
        TaskPhase::Complete,
        "a task that ran to the end reports Complete, not Canceled"
    );
}

#[test]
fn cancelling_outranks_a_pending_wakeup() {
    let scheduler = Scheduler::new();
    let join = scheduler.spawn(Park);
    let task = join.task_ref();
    assert!(task.status().is_woken(), "it is queued to be polled");
    task.cancel();

    let status = task.status();
    assert!(status.is_woken(), "the wakeup is still recorded");
    assert_eq!(
        status.phase(),
        TaskPhase::Canceled,
        "cancellation is reported ahead of the pending wakeup"
    );
}

#[test]
fn every_phase_has_a_stable_lowercase_name() {
    for (phase, name) in [
        (TaskPhase::Complete, "complete"),
        (TaskPhase::Canceled, "canceled"),
        (TaskPhase::Running, "running"),
        (TaskPhase::Scheduled, "scheduled"),
        (TaskPhase::Idle, "idle"),
    ] {
        let text: &'static str = phase.as_str();
        assert_eq!(text, name);
        assert_eq!(format!("{phase}"), name, "Display writes the same name");
    }
}

#[test]
fn task_phase_is_a_copyable_comparable_value() {
    let phase = TaskPhase::Running;
    let copied = phase;
    assert_eq!(phase, copied);
    assert_ne!(TaskPhase::Idle, TaskPhase::Scheduled);
    let _ = format!("{:?}", TaskPhase::Complete);
}
