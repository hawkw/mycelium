#![cfg(all(feature = "alloc", not(loom)))]

use maitake::{
    scheduler::Scheduler,
    task::{TaskObserver, TaskPhase, TaskStatus},
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

struct Park;
impl Future for Park {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        Poll::Pending
    }
}

fn controlled(s: &Scheduler) -> (maitake::task::JoinHandle<()>, Arc<Mutex<Option<Waker>>>) {
    let saved = Arc::new(Mutex::new(None));
    let copy = saved.clone();
    let j = s.spawn(std::future::poll_fn(move |cx| {
        *copy.lock().unwrap() = Some(cx.waker().clone());
        Poll::<()>::Pending
    }));
    (j, saved)
}
fn wake(saved: &Arc<Mutex<Option<Waker>>>) {
    saved.lock().unwrap().as_ref().unwrap().wake_by_ref();
}

#[derive(Default)]
struct Counter(AtomicUsize);
impl Counter {
    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn poll<F: Future>(f: Pin<&mut F>, counter: &Arc<Counter>) -> Poll<F::Output> {
    f.poll(&mut Context::from_waker(&Waker::from(counter.clone())))
}
fn next(o: &mut TaskObserver) -> Option<TaskStatus> {
    let mut f = Box::pin(o.changed());
    match poll(f.as_mut(), &Arc::default()) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("expected immediate delivery"),
    }
}
fn pending(o: &mut TaskObserver) {
    let mut f = Box::pin(o.changed());
    let w = Arc::default();
    assert!(poll(f.as_mut(), &w).is_pending());
    assert_eq!(w.count(), 0, "waiting must not wake itself");
}

#[test]
fn initial_delivery_samples_when_polled() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut o = j.task_ref().observe();
    let mut f = Box::pin(o.changed());
    s.tick();
    let status = match poll(f.as_mut(), &Arc::default()) {
        Poll::Ready(Some(s)) => s,
        _ => panic!("initial snapshot missing"),
    };
    assert_eq!(status.phase(), TaskPhase::Idle);
}

#[test]
fn observer_is_quiet_after_initial_delivery() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut o = j.task_ref().observe();
    assert_eq!(next(&mut o).unwrap().phase(), TaskPhase::Scheduled);
    let mut f = Box::pin(o.changed());
    let w = Arc::default();
    for _ in 0..3 {
        assert!(poll(f.as_mut(), &w).is_pending());
    }
    assert_eq!(w.count(), 0);
}

#[test]
fn pending_poll_notifies_all_observers() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let t = j.task_ref();
    let mut a = t.observe();
    let mut b = t.observe();
    next(&mut a);
    next(&mut b);
    let mut fa = Box::pin(a.changed());
    let mut fb = Box::pin(b.changed());
    let wa = Arc::default();
    let wb = Arc::default();
    assert!(poll(fa.as_mut(), &wa).is_pending());
    assert!(poll(fb.as_mut(), &wb).is_pending());
    s.tick();
    assert!(wa.count() > 0 && wb.count() > 0);
    for result in [poll(fa.as_mut(), &wa), poll(fb.as_mut(), &wb)] {
        assert!(matches!(result, Poll::Ready(Some(v)) if v.phase() == TaskPhase::Idle));
    }
}

#[test]
fn unchanged_status_poll_is_still_progress() {
    let s = Scheduler::new();
    let (j, saved) = controlled(&s);
    let t = j.task_ref();
    s.tick();
    let mut o = t.observe();
    let before = next(&mut o).unwrap();
    wake(&saved);
    s.tick();
    let after = next(&mut o).unwrap();
    assert_eq!(
        before, after,
        "fixture returns to exactly the same public status"
    );
    pending(&mut o);
}

#[test]
fn wake_and_reference_and_join_changes_are_not_events() {
    let s = Scheduler::new();
    let (j, saved) = controlled(&s);
    let t = j.task_ref();
    s.tick();
    let mut o = t.observe();
    next(&mut o);
    let c = t.clone();
    drop(c);
    drop(j);
    wake(&saved);
    assert_eq!(o.status().phase(), TaskPhase::Scheduled);
    assert!(!o.status().has_join_handle());
    pending(&mut o);
    s.tick();
    assert_eq!(next(&mut o).unwrap().phase(), TaskPhase::Idle);
    pending(&mut o);
}

#[test]
fn completed_events_coalesce_and_status_does_not_acknowledge() {
    let s = Scheduler::new();
    let (j, saved) = controlled(&s);
    let t = j.task_ref();
    let mut o = t.observe();
    next(&mut o);
    s.tick();
    for _ in 0..4 {
        wake(&saved);
        s.tick();
    }
    assert_eq!(o.status().phase(), TaskPhase::Idle);
    assert_eq!(next(&mut o).unwrap().phase(), TaskPhase::Idle);
    pending(&mut o);
}

#[test]
fn completion_preserves_join_notification_and_noncopy_output() {
    let s = Scheduler::new();
    let j = s.spawn(async { String::from("owned output") });
    let t = j.task_ref();
    let mut o = t.observe();
    next(&mut o);
    let mut joined = tokio_test::task::spawn(j);
    assert!(joined.poll().is_pending());
    let mut f = Box::pin(o.changed());
    let w = Arc::default();
    assert!(poll(f.as_mut(), &w).is_pending());
    s.tick();
    assert!(w.count() > 0);
    assert!(joined.is_woken());
    assert!(
        matches!(poll(f.as_mut(), &w), Poll::Ready(Some(v)) if v.is_completed() && v.has_output())
    );
    assert_eq!(tokio_test::assert_ready_ok!(joined.poll()), "owned output");
    assert!(!t.status().has_output());
}

#[test]
fn cancellation_notifies_before_scheduler_cleanup() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let t = j.task_ref();
    let mut o = t.observe();
    next(&mut o);
    let mut f = Box::pin(o.changed());
    let w = Arc::default();
    assert!(poll(f.as_mut(), &w).is_pending());
    assert!(j.cancel());
    assert!(!j.cancel());
    assert!(w.count() > 0);
    assert!(
        matches!(poll(f.as_mut(), &w), Poll::Ready(Some(v)) if v.is_canceled() && !v.is_completed())
    );
}

#[test]
fn other_tasks_do_not_satisfy_a_wait() {
    let s = Scheduler::new();
    let (ja, saved) = controlled(&s);
    let jb = s.spawn(Park);
    s.tick();
    let mut a = ja.task_ref().observe();
    next(&mut a);
    jb.cancel();
    pending(&mut a);
    wake(&saved);
    s.tick();
    assert_eq!(next(&mut a).unwrap().phase(), TaskPhase::Idle);
}

#[test]
fn latest_waker_receives_notification() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut o = j.task_ref().observe();
    next(&mut o);
    let mut f = Box::pin(o.changed());
    let old = Arc::default();
    let latest = Arc::default();
    assert!(poll(f.as_mut(), &old).is_pending());
    assert!(poll(f.as_mut(), &latest).is_pending());
    s.tick();
    assert!(latest.count() > 0);
    assert!(matches!(poll(f.as_mut(), &latest), Poll::Ready(Some(_))));
}

#[test]
fn dropped_wait_unregisters_and_preserves_progress() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut o = j.task_ref().observe();
    next(&mut o);
    let w = Arc::default();
    {
        let mut f = Box::pin(o.changed());
        assert!(poll(f.as_mut(), &w).is_pending());
    }
    s.tick();
    assert_eq!(w.count(), 0, "dropped wait must be unregistered");
    assert_eq!(next(&mut o).unwrap().phase(), TaskPhase::Idle);
    pending(&mut o);
}

#[test]
fn notified_but_unpolled_wait_can_be_recreated() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut o = j.task_ref().observe();
    next(&mut o);
    {
        let mut f = Box::pin(o.changed());
        let w = Arc::default();
        assert!(poll(f.as_mut(), &w).is_pending());
        s.tick();
        assert!(w.count() > 0);
    }
    assert_eq!(next(&mut o).unwrap().phase(), TaskPhase::Idle);
    pending(&mut o);
}

#[test]
fn cloning_inherits_acknowledged_cursor() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut a = j.task_ref().observe();
    next(&mut a);
    let mut b = a.clone();
    pending(&mut b);
    s.tick();
    next(&mut a);
    next(&mut b);
    pending(&mut a);
    pending(&mut b);
}

#[test]
fn cloning_preserves_unacknowledged_progress() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut a = j.task_ref().observe();
    next(&mut a);
    s.tick();
    let mut b = a.clone();
    next(&mut a);
    next(&mut b);
    pending(&mut a);
    pending(&mut b);
}

#[test]
fn terminal_delivery_ends_each_cursor_and_its_clones() {
    let s = Scheduler::new();
    let j = s.spawn(async {});
    let t = j.task_ref();
    let mut o = t.observe();
    next(&mut o);
    s.tick();
    assert!(next(&mut o).unwrap().is_completed());
    let mut c = o.clone();
    for _ in 0..3 {
        assert!(next(&mut o).is_none());
        assert!(next(&mut c).is_none());
    }
    t.cancel();
    assert!(o.status().is_canceled());
    assert!(next(&mut o).is_none());
}

#[test]
fn late_observers_deliver_finished_status_once_after_join() {
    let s = Scheduler::new();
    let j = s.spawn(async { 91 });
    let t = j.task_ref();
    s.tick();
    let mut joined = tokio_test::task::spawn(j);
    assert_eq!(tokio_test::assert_ready_ok!(joined.poll()), 91);
    let mut o = t.observe();
    let v = next(&mut o).unwrap();
    assert!(v.is_completed() && !v.has_output());
    assert!(next(&mut o).is_none());
}

#[test]
fn observer_reference_accounting_and_snapshot_lifetime() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let t = j.task_ref();
    let initial = t.status();
    let o = t.observe();
    assert_eq!(t.status().ref_count(), initial.ref_count() + 1);
    let c = o.clone();
    assert_eq!(t.status().ref_count(), initial.ref_count() + 2);
    let snapshot = c.status();
    drop(c);
    drop(o);
    assert_eq!(t.status().ref_count(), initial.ref_count());
    assert_eq!(snapshot.ref_count(), initial.ref_count() + 2);
    assert!(!t.status().is_canceled());
    s.tick();
    drop(j);
    drop(t);
    assert_eq!(initial.phase(), TaskPhase::Scheduled);
}

#[test]
fn dropping_one_observer_preserves_other_waiter_and_join() {
    let s = Scheduler::new();
    let j = s.spawn(async { 5 });
    let t = j.task_ref();
    let mut a = t.observe();
    let mut b = t.observe();
    next(&mut a);
    next(&mut b);
    let wa = Arc::default();
    let wb = Arc::default();
    {
        let mut f = Box::pin(a.changed());
        assert!(poll(f.as_mut(), &wa).is_pending());
    }
    drop(a);
    let mut f = Box::pin(b.changed());
    assert!(poll(f.as_mut(), &wb).is_pending());
    s.tick();
    assert!(wb.count() > 0);
    assert_eq!(wa.count(), 0);
    assert!(!t.status().is_canceled());
    assert!(matches!(poll(f.as_mut(), &wb), Poll::Ready(Some(v)) if v.is_completed()));
    assert_eq!(
        tokio_test::assert_ready_ok!(tokio_test::task::spawn(j).poll()),
        5
    );
}

#[test]
fn running_woken_and_canceled_facts_compose() {
    let s = Scheduler::new();
    let inner = s.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capture = seen.clone();
    let j = s.spawn(std::future::poll_fn(move |cx| {
        let t = inner.current_task().unwrap();
        cx.waker().wake_by_ref();
        capture.lock().unwrap().push(t.status());
        t.cancel();
        capture.lock().unwrap().push(t.status());
        Poll::Ready(17)
    }));
    let t = j.task_ref();
    s.tick();
    let seen = seen.lock().unwrap();
    assert!(seen[0].is_polling() && seen[0].is_woken());
    assert_eq!(seen[0].phase(), TaskPhase::Running);
    assert!(seen[1].is_polling() && seen[1].is_canceled() && !seen[1].is_completed());
    assert_eq!(seen[1].phase(), TaskPhase::Canceled);
    let final_state = t.status();
    assert!(final_state.is_completed() && final_state.is_canceled() && final_state.has_output());
    assert_eq!(final_state.phase(), TaskPhase::Complete);
    let mut joined = tokio_test::task::spawn(j);
    assert!(matches!(joined.poll(), Poll::Ready(Err(_))));
    assert!(joined.into_inner().status().is_none());
    assert!(!t.status().has_output());
}

#[test]
fn status_and_observer_support_local_non_send_outputs() {
    use std::rc::Rc;
    let s = maitake::scheduler::LocalScheduler::new();
    let j = s.spawn(async { Rc::new(23) });
    let mut o = j.task_ref().observe();
    fn send_sync<T: Send + Sync + std::fmt::Debug>(_: &T) {}
    fn send<T: Send>(_: &T) {}
    send_sync(&o);
    send(&o.changed());
    next(&mut o);
    s.tick();
    assert!(next(&mut o).unwrap().is_completed());
    assert_eq!(
        *tokio_test::assert_ready_ok!(tokio_test::task::spawn(j).poll()),
        23
    );
}

#[test]
fn canceled_delivery_ends_the_cursor() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut o = j.task_ref().observe();
    next(&mut o);
    j.cancel();
    assert!(next(&mut o).unwrap().is_canceled());
    for _ in 0..2 {
        assert!(next(&mut o).is_none());
    }
    s.tick();
    assert!(next(&mut o).is_none());
}

#[test]
fn cloning_an_unread_observer_preserves_initial_delivery() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let mut a = j.task_ref().observe();
    let mut b = a.clone();
    assert_eq!(next(&mut a).unwrap().phase(), TaskPhase::Scheduled);
    assert_eq!(next(&mut b).unwrap().phase(), TaskPhase::Scheduled);
    pending(&mut a);
    pending(&mut b);
}

#[test]
fn late_canceled_observer_delivers_before_ending() {
    let s = Scheduler::new();
    let j = s.spawn(Park);
    let t = j.task_ref();
    j.cancel();
    let mut o = t.observe();
    let first = next(&mut o).expect("a late observer still has an initial delivery");
    assert!(first.is_canceled() && !first.is_completed());
    assert!(next(&mut o).is_none());
}
