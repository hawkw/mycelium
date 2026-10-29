#![cfg(loom)]
use crate::scheduler::Scheduler;
use loom::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn observer_notification_allows_reentrant_cancellation() {
    struct CancelOnWake {
        task: crate::task::TaskRef,
        calls: AtomicUsize,
    }
    impl Wake for CancelOnWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                assert!(self.task.cancel());
            }
        }
    }

    loom::model(|| {
        let scheduler = Scheduler::new();
        let join = scheduler.spawn(std::future::pending::<()>());
        let task = join.task_ref();
        let mut observer = task.observe();
        let callback = Arc::new(CancelOnWake {
            task: task.clone(),
            calls: AtomicUsize::new(0),
        });
        let waker = Waker::from(callback.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(
            Box::pin(observer.changed()).as_mut().poll(&mut cx),
            Poll::Ready(Some(_))
        ));
        let mut changed = Box::pin(observer.changed());
        assert!(changed.as_mut().poll(&mut cx).is_pending());

        scheduler.tick();

        assert!(callback.calls.load(Ordering::SeqCst) > 0);
        let status = match changed.as_mut().poll(&mut cx) {
            Poll::Ready(Some(status)) => status,
            other => panic!("expected cancellation delivery, got {other:?}"),
        };
        assert!(status.is_canceled());
        drop(changed);
        assert!(matches!(
            Box::pin(observer.changed()).as_mut().poll(&mut cx),
            Poll::Ready(None)
        ));
    });
}

#[test]
fn progress_overlapping_registration_is_delivered() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    model.max_permutations = Some(5000);
    model.check(|| {
        let scheduler = Scheduler::new();
        let join = scheduler.spawn(std::future::pending::<()>());
        let task = join.task_ref();
        let mut observer = task.observe();
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(
            Box::pin(observer.changed()).as_mut().poll(&mut cx),
            Poll::Ready(Some(_))
        ));
        let worker = thread::spawn(move || {
            scheduler.tick();
        });
        let mut changed = Box::pin(observer.changed());
        let first = changed.as_mut().poll(&mut cx);
        worker.join().unwrap();
        if first.is_pending() {
            assert!(
                count.0.load(Ordering::SeqCst) > 0,
                "progress lost during registration"
            );
            assert!(matches!(
                changed.as_mut().poll(&mut cx),
                Poll::Ready(Some(_))
            ));
        } else {
            assert!(matches!(first, Poll::Ready(Some(_))));
        }
    });
}

#[test]
fn snapshot_does_not_mix_before_and_after_completion() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = None;
    model.checkpoint_interval = 1;
    model.max_permutations = Some(5000);
    model.check(|| {
        let scheduler = Scheduler::new();
        let (entered_tx, entered_rx) = loom::sync::mpsc::channel();
        let (finish_tx, finish_rx) = loom::sync::mpsc::channel();
        let join = scheduler.spawn(async move {
            entered_tx.send(()).unwrap();
            finish_rx.recv().unwrap();
            17u8
        });
        let task = join.task_ref();
        let worker = thread::spawn(move || {
            scheduler.tick();
        });
        entered_rx.recv().unwrap();
        finish_tx.send(()).unwrap();
        thread::yield_now();
        let snapshot = task.status();
        // This scenario keeps the join handle alive and never polls it, so
        // completion and output availability transition together.
        assert_eq!(snapshot.is_completed(), snapshot.has_output());
        worker.join().unwrap();
        let final_status = task.status();
        assert!(final_status.is_completed());
        assert!(final_status.has_output());
        drop(join);
    });
}
