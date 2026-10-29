#![cfg(all(feature = "alloc", not(loom)))]

use maitake::{
    scheduler::Scheduler,
    task::{TaskMonitor, TaskPhase, TaskRef, TaskUpdate},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn poll<F: Future>(f: Pin<&mut F>, w: &Arc<Count>) -> Poll<F::Output> {
    f.poll(&mut Context::from_waker(&Waker::from(w.clone())))
}
fn ready<F: Future>(f: F) -> F::Output {
    match poll(Box::pin(f).as_mut(), &Arc::default()) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("expected immediate result"),
    }
}
fn pending(m: &mut TaskMonitor) {
    let w = Arc::default();
    assert!(poll(Box::pin(m.next()).as_mut(), &w).is_pending());
    assert_eq!(w.0.load(Ordering::SeqCst), 0);
}
fn parked(s: &Scheduler) -> TaskRef {
    s.spawn(std::future::pending::<()>()).task_ref()
}
fn controlled(s: &Scheduler) -> (TaskRef, Arc<Mutex<Option<Waker>>>) {
    let saved = Arc::new(Mutex::new(None));
    let copy = saved.clone();
    let task = s
        .spawn(std::future::poll_fn(move |cx| {
            *copy.lock().unwrap() = Some(cx.waker().clone());
            Poll::<()>::Pending
        }))
        .task_ref();
    (task, saved)
}
fn ids(updates: Vec<TaskUpdate>) -> Vec<maitake::task::TaskId> {
    updates.into_iter().map(|u| u.id).collect()
}

#[test]
fn exact_capacity_deduplicates_before_validating() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone(), a.clone(), b.clone()])
        .unwrap();
    assert_eq!(m.capacity(), 2);
    assert_eq!(m.len(), 2);
    assert_eq!(m.active_len(), 2);
    assert_eq!(m.subscribe(&a), Ok(false));
    assert_eq!(ids(m.snapshot()), [a.id(), b.id()]);
    let err = m.subscribe(&c).unwrap_err();
    assert_eq!((err.capacity(), err.required()), (2, 3));
    let err2 = m
        .reconcile(&[a.clone(), c.clone(), b.clone(), c.clone()])
        .unwrap_err();
    assert_eq!(err, err2);
    fn traits<T: Copy + Clone + std::fmt::Debug + Eq + PartialEq + std::fmt::Display>(_: T) {}
    traits(err);
}

#[test]
fn zero_capacity_and_empty_reconciliation_are_reusable() {
    let s = Scheduler::new();
    let a = parked(&s);
    let before = a.status().ref_count();
    let mut m = TaskMonitor::new(0);
    m.reconcile(&[]).unwrap();
    assert_eq!(m.subscribe(&a).unwrap_err().required(), 1);
    assert_eq!(
        m.reconcile(&[a.clone(), a.clone()]).unwrap_err().required(),
        1
    );
    assert!(m.is_empty());
    assert_eq!(m.active_len(), 0);
    assert!(ready(m.next()).is_none());
    assert!(ready(m.next_batch(usize::MAX)).is_empty());
    assert_eq!(a.status().ref_count(), before);
}

#[test]
fn failed_reconciliation_preserves_order_ownership_progress_and_priority() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    let counts = [
        a.status().ref_count(),
        b.status().ref_count(),
        c.status().ref_count(),
    ];
    assert!(m.reconcile(&[c.clone(), b.clone(), a.clone()]).is_err());
    assert_eq!(ids(m.snapshot()), [a.id(), b.id()]);
    assert_eq!(
        counts,
        [
            a.status().ref_count(),
            b.status().ref_count(),
            c.status().ref_count()
        ]
    );
    assert_eq!(ready(m.next()).unwrap().id, b.id());
    pending(&mut m);
    s.tick();
    assert!(m.subscribe(&c).is_err());
    assert_eq!(ids(ready(m.next_batch(2))), [a.id(), b.id()]);
}

#[test]
fn reconciliation_preserves_unread_and_acknowledged_cursors() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let mut m = TaskMonitor::new(3);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    m.reconcile(&[b.clone(), a.clone(), b.clone(), c.clone()])
        .unwrap();
    assert_eq!(ids(ready(m.next_batch(10))), [b.id(), c.id()]);
    pending(&mut m);
    s.tick();
    m.reconcile(&[c.clone(), b.clone(), a.clone()]).unwrap();
    assert_eq!(ids(ready(m.next_batch(3))), [c.id(), b.id(), a.id()]);

    // Even an identical replacement restarts selection, without resetting cursors.
    let scheduler = Scheduler::new();
    let first = parked(&scheduler);
    let second = parked(&scheduler);
    let mut same = TaskMonitor::new(2);
    same.reconcile(&[first.clone(), second.clone()]).unwrap();
    assert_eq!(ready(same.next()).unwrap().id, first.id());
    scheduler.tick();
    same.reconcile(&[first.clone(), second.clone(), first.clone()])
        .unwrap();
    assert_eq!(ready(same.next()).unwrap().id, first.id());
    assert_eq!(ready(same.next()).unwrap().id, second.id());
    pending(&mut same);
}

#[test]
fn terminal_members_use_capacity_until_removed_and_readd_gets_initial() {
    let s = Scheduler::new();
    let j = s.spawn(async { String::from("kept") });
    let a = j.task_ref();
    let b = parked(&s);
    let mut m = TaskMonitor::new(1);
    assert_eq!(m.subscribe(&a), Ok(true));
    s.tick();
    assert_eq!(m.active_len(), 1);
    let update = ready(m.next()).unwrap();
    assert!(update.status.is_completed());
    assert_eq!((m.len(), m.active_len()), (1, 0));
    assert!(m.subscribe(&b).is_err());
    assert_eq!(m.subscribe(&a), Ok(false));
    m.reconcile(&[a.clone(), a.clone()]).unwrap();
    assert!(ready(m.next()).is_none());
    assert!(m.status(a.id()).unwrap().has_output());
    assert_eq!(ready(j).unwrap(), "kept");
    assert!(!m.snapshot()[0].status.has_output());
    assert!(m.remove(a.id()));
    assert!(!m.remove(a.id()));
    assert_eq!(m.subscribe(&a), Ok(true));
    assert_eq!(m.active_len(), 1);
    assert!(ready(m.next()).unwrap().status.is_completed());
}

#[test]
fn zero_limit_changes_neither_delivery_nor_priority() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert!(ready(m.next_batch(0)).is_empty());
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    assert!(ready(m.next_batch(0)).is_empty());
    assert_eq!(ready(m.next()).unwrap().id, b.id());
    assert!(ready(m.next_batch(0)).is_empty());
    pending(&mut m);
}

#[test]
fn bounded_batches_preserve_unselected_events_and_rotate() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let mut m = TaskMonitor::new(3);
    m.reconcile(&[a.clone(), b.clone(), c.clone()]).unwrap();
    assert_eq!(ids(ready(m.next_batch(2))), [a.id(), b.id()]);
    s.tick();
    assert_eq!(ids(ready(m.next_batch(2))), [c.id(), a.id()]);
    assert_eq!(ids(ready(m.next_batch(2))), [b.id()]);
    pending(&mut m);
}

#[test]
fn full_cycle_visits_each_ready_member_once_and_large_limit_does_not_wait() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(ids(ready(m.next_batch(usize::MAX))), [a.id(), b.id()]);
    pending(&mut m);
}

#[test]
fn appending_and_removing_preserve_successor_priority() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let d = parked(&s);
    let mut m = TaskMonitor::new(4);
    m.reconcile(&[a.clone(), b.clone(), c.clone()]).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    m.subscribe(&d).unwrap();
    s.tick();
    assert!(m.remove(a.id()));
    assert_eq!(ready(m.next()).unwrap().id, b.id());
    assert!(m.remove(c.id()));
    assert_eq!(ready(m.next()).unwrap().id, d.id());
    pending(&mut m);
}

#[test]
fn removing_last_priority_wraps_and_missing_removal_is_noop() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let absent = parked(&s);
    let mut m = TaskMonitor::new(3);
    m.reconcile(&[a.clone(), b.clone(), c.clone()]).unwrap();
    ready(m.next_batch(2));
    s.tick();
    assert!(!m.remove(absent.id()));
    assert!(m.remove(c.id()));
    assert_eq!(ready(m.next()).unwrap().id, a.id());
}

#[test]
fn inspection_never_acknowledges_and_snapshots_own_no_references() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    let before = a.status().ref_count();
    let snap = m.snapshot();
    assert_eq!(a.status().ref_count(), before);
    assert!(m.status(b.id()).is_some());
    assert_eq!(ids(ready(m.next_batch(2))), [a.id(), b.id()]);
    s.tick();
    assert_eq!(snap[0].status.phase(), TaskPhase::Scheduled);
    assert_eq!(m.status(a.id()).unwrap().phase(), TaskPhase::Idle);
    assert_eq!(ids(ready(m.next_batch(2))), [a.id(), b.id()]);
    m.clear();
    assert!(m.status(a.id()).is_none());
}

#[test]
fn clone_preserves_cursor_priority_and_exact_ownership_independently() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let before = a.status().ref_count();
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(a.status().ref_count(), before + 1);
    ready(m.next());
    let mut copy = m.clone();
    assert_eq!(a.status().ref_count(), before + 2);
    assert_eq!(ready(copy.next()).unwrap().id, b.id());
    assert_eq!(ready(m.next()).unwrap().id, b.id());
    pending(&mut copy);
    pending(&mut m);
    drop(copy);
    assert_eq!(a.status().ref_count(), before + 1);
    m.clear();
    assert_eq!(a.status().ref_count(), before);
    m.subscribe(&a).unwrap();
    assert!(ready(m.next()).is_some());
    drop(m);
    assert_eq!(a.status().ref_count(), before);
    assert!(!a.status().is_canceled());
}

#[test]
fn pending_batch_registers_every_member_across_schedulers_and_latest_waker() {
    let a_sched = Scheduler::new();
    let b_sched = Scheduler::new();
    let a = parked(&a_sched);
    let b = parked(&b_sched);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    ready(m.next_batch(2));
    let old = Arc::default();
    let latest = Arc::default();
    let mut f = Box::pin(m.next_batch(2));
    assert!(poll(f.as_mut(), &old).is_pending());
    assert!(poll(f.as_mut(), &latest).is_pending());
    assert_eq!(latest.0.load(Ordering::SeqCst), 0);
    b_sched.tick();
    assert!(latest.0.load(Ordering::SeqCst) > 0);
    assert_eq!(
        ids(match poll(f.as_mut(), &latest) {
            Poll::Ready(v) => v,
            _ => panic!("second scheduler lost"),
        }),
        [b.id()]
    );
    drop(f);
    pending(&mut m);
}

#[test]
fn dropped_batch_unregisters_all_and_replays_notified_progress() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    ready(m.next_batch(2));
    let before = a.status().ref_count();
    let w = Arc::default();
    let counts = [a.status().ref_count(), b.status().ref_count()];
    let mut single = Box::pin(m.next());
    assert!(poll(single.as_mut(), &w).is_pending());
    assert_eq!([a.status().ref_count(), b.status().ref_count()], counts);
    drop(single);
    let mut f = Box::pin(m.next_batch(2));
    assert!(poll(f.as_mut(), &w).is_pending());
    assert_eq!(a.status().ref_count(), before);
    drop(f);
    s.tick();
    assert_eq!(w.0.load(Ordering::SeqCst), 0);
    assert_eq!(ids(ready(m.next_batch(2))), [a.id(), b.id()]);
    let mut f = Box::pin(m.next_batch(1));
    assert!(poll(f.as_mut(), &w).is_pending());
    b.cancel();
    assert!(w.0.load(Ordering::SeqCst) > 0);
    drop(f);
    assert_eq!(ready(m.next()).unwrap().id, b.id());
}

#[test]
fn empty_and_canceled_sets_reopen_without_ghost_notifications() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(1);
    assert!(ready(m.next()).is_none());
    m.subscribe(&a).unwrap();
    a.cancel();
    assert!(ready(m.next()).unwrap().status.is_canceled());
    assert!(ready(m.next()).is_none());
    m.reconcile(&[]).unwrap();
    m.subscribe(&b).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, b.id());
    let w = Arc::default();
    let mut f = Box::pin(m.next());
    assert!(poll(f.as_mut(), &w).is_pending());
    drop(f);
    m.clear();
    s.tick();
    assert_eq!(w.0.load(Ordering::SeqCst), 0);
    assert!(ready(m.next_batch(1)).is_empty());
}

#[test]
fn hot_member_does_not_starve_a_later_ready_member() {
    let s = Scheduler::new();
    let (a, wake) = controlled(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    s.tick();
    assert_eq!(ready(m.next()).unwrap().id, b.id());
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    wake.lock().unwrap().as_ref().unwrap().wake_by_ref();
    s.tick();
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    pending(&mut m);
}

#[test]
fn monitor_supports_non_send_outputs_and_send_futures() {
    fn value<T: Copy + Clone + std::fmt::Debug + Eq + PartialEq>() {}
    value::<TaskUpdate>();
    fn send_sync<T: Send + Sync + Clone + std::fmt::Debug>() {}
    send_sync::<TaskMonitor>();
    fn send<T: Send>(_: &T) {}
    let s = maitake::scheduler::LocalScheduler::new();
    let j = s.spawn(async { std::rc::Rc::new(String::from("owned")) });
    let a = j.task_ref();
    let mut m = TaskMonitor::new(1);
    m.subscribe(&a).unwrap();
    {
        let f = m.next_batch(1);
        send(&f);
    }
    {
        let f = m.next();
        send(&f);
    }
    s.tick();
    assert!(ready(m.next()).unwrap().status.has_output());
    let output = ready(j).unwrap();
    assert_eq!(output.as_str(), "owned");
    assert!(!m.status(a.id()).unwrap().has_output());
}

#[test]
fn clone_preserves_rotated_priority_with_multiple_ready_tasks() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    ready(m.next());
    s.tick();
    let mut copy = m.clone();
    assert_eq!(ids(ready(copy.next_batch(2))), [b.id(), a.id()]);
    assert_eq!(ids(ready(m.next_batch(2))), [b.id(), a.id()]);
}

#[test]
fn clone_and_reconcile_keep_delivered_terminal_members_exhausted() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    a.cancel();
    ready(m.next_batch(2));
    let mut copy = m.clone();
    assert_eq!(copy.active_len(), 1);
    copy.reconcile(&[a.clone()]).unwrap();
    assert_eq!(copy.active_len(), 0);
    assert!(ready(copy.next()).is_none());
    pending(&mut m);
}

#[test]
fn successful_reconciliation_releases_removed_members_only() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let before = [
        a.status().ref_count(),
        b.status().ref_count(),
        c.status().ref_count(),
    ];
    let mut m = TaskMonitor::new(2);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    m.reconcile(&[b.clone(), c.clone(), b.clone()]).unwrap();
    assert_eq!(
        [
            a.status().ref_count(),
            b.status().ref_count(),
            c.status().ref_count()
        ],
        [before[0], before[1] + 1, before[2] + 1]
    );
    assert!(!a.status().is_canceled());
    assert_eq!(ids(m.snapshot()), [b.id(), c.id()]);
    assert!(m.status(a.id()).is_none());
}

#[test]
fn builder_and_injector_start_scheduled_on_every_status_surface() {
    let scheduler = Scheduler::new();
    let builder = scheduler.build_task().spawn(std::future::pending::<()>());
    let injector = maitake::scheduler::Injector::<Scheduler>::new();
    let injected = injector.spawn(std::future::pending::<()>());
    for join in [builder, injected] {
        assert!(join.status().unwrap().is_woken());
        let task = join.task_ref();
        assert_eq!(task.status().phase(), TaskPhase::Scheduled);
        let mut observer = task.observe();
        assert_eq!(
            ready(observer.changed()).unwrap().phase(),
            TaskPhase::Scheduled
        );
        let mut monitor = TaskMonitor::new(1);
        monitor.subscribe(&task).unwrap();
        assert_eq!(
            ready(monitor.next()).unwrap().status.phase(),
            TaskPhase::Scheduled
        );
    }
}

#[test]
fn required_count_reports_the_whole_distinct_request() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let d = parked(&s);
    let mut m = TaskMonitor::new(2);
    m.subscribe(&b).unwrap();
    let error = m
        .reconcile(&[a.clone(), b.clone(), c.clone(), d.clone(), a.clone()])
        .unwrap_err();
    assert_eq!((error.capacity(), error.required()), (2, 4));
    assert_eq!(ids(m.snapshot()), [b.id()]);
    assert_eq!(ready(m.next()).unwrap().id, b.id());
}

#[test]
fn removal_preserves_priority_among_multiple_ready_survivors() {
    for remove_next in [false, true] {
        let s = Scheduler::new();
        let a = parked(&s);
        let b = parked(&s);
        let c = parked(&s);
        let mut m = TaskMonitor::new(3);
        m.reconcile(&[a.clone(), b.clone(), c.clone()]).unwrap();
        assert_eq!(ready(m.next()).unwrap().id, a.id());
        s.tick();
        let (removed, expected) = if remove_next {
            (b.id(), c.id())
        } else {
            (c.id(), b.id())
        };
        assert!(m.remove(removed));
        assert_eq!(ready(m.next()).unwrap().id, expected);
        assert_eq!(ready(m.next()).unwrap().id, a.id());
    }
}

#[test]
fn removing_membership_leaves_task_and_join_usable() {
    let s = Scheduler::new();
    let join = s.spawn(async { String::from("still running") });
    let task = join.task_ref();
    let before = task.status().ref_count();
    let mut m = TaskMonitor::new(1);
    m.subscribe(&task).unwrap();
    assert!(m.remove(task.id()));
    assert_eq!(task.status().ref_count(), before);
    assert!(!task.status().is_canceled());
    s.tick();
    assert_eq!(ready(join).unwrap(), "still running");
}

#[test]
fn capacity_and_emptiness_describe_retained_membership() {
    let s = Scheduler::new();
    let a = parked(&s);
    let mut m = TaskMonitor::new(3);
    assert_eq!(m.capacity(), 3);
    assert!(m.is_empty());
    m.subscribe(&a).unwrap();
    assert_eq!(m.capacity(), 3);
    assert!(!m.is_empty());
    a.cancel();
    ready(m.next());
    assert_eq!(m.len(), 1);
    assert_eq!(m.active_len(), 0);
    assert!(!m.is_empty());
    assert!(m.remove(a.id()));
    assert!(m.is_empty());
    assert_eq!(m.capacity(), 3);
}

#[test]
fn duplicate_subscription_preserves_cursor_and_rotation() {
    for make_progress in [false, true] {
        let s = Scheduler::new();
        let a = parked(&s);
        let b = parked(&s);
        let c = parked(&s);
        let mut m = TaskMonitor::new(3);
        m.reconcile(&[a.clone(), b.clone(), c.clone()]).unwrap();
        assert_eq!(ready(m.next()).unwrap().id, a.id());
        if make_progress {
            s.tick();
        }
        let before = a.status().ref_count();
        assert_eq!(m.subscribe(&a), Ok(false));
        assert_eq!(a.status().ref_count(), before);
        let expected = if make_progress {
            vec![b.id(), c.id(), a.id()]
        } else {
            vec![b.id(), c.id()]
        };
        assert_eq!(ids(ready(m.next_batch(3))), expected);
        pending(&mut m);
    }
}

#[test]
fn appending_preserves_priority_with_ready_existing_members() {
    let s = Scheduler::new();
    let a = parked(&s);
    let b = parked(&s);
    let c = parked(&s);
    let mut m = TaskMonitor::new(3);
    m.reconcile(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, a.id());
    s.tick();
    m.subscribe(&c).unwrap();
    assert_eq!(ids(ready(m.next_batch(3))), [b.id(), c.id(), a.id()]);
}

#[test]
fn monitor_filters_non_events_and_coalesces_poll_progress() {
    let s = Scheduler::new();
    let saved = Arc::new(Mutex::new(None));
    let copy = saved.clone();
    let join = s.spawn(std::future::poll_fn(move |cx| {
        *copy.lock().unwrap() = Some(cx.waker().clone());
        Poll::<()>::Pending
    }));
    let task = join.task_ref();
    s.tick();
    let mut m = TaskMonitor::new(1);
    m.subscribe(&task).unwrap();
    assert_eq!(ready(m.next()).unwrap().id, task.id());
    pending(&mut m);

    let extra = task.clone();
    pending(&mut m);
    drop(extra);
    pending(&mut m);
    drop(join);
    pending(&mut m);
    saved.lock().unwrap().as_ref().unwrap().wake_by_ref();
    pending(&mut m);

    // Actual polls, including cycles back to Idle, coalesce for this monitor.
    s.tick();
    for _ in 0..3 {
        saved.lock().unwrap().as_ref().unwrap().wake_by_ref();
        s.tick();
    }
    assert_eq!(ids(ready(m.next_batch(4))), [task.id()]);
    pending(&mut m);
}
