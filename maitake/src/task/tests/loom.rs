use super::*;
use crate::{
    loom::{
        self,
        alloc::{Track, TrackFuture},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    },
    scheduler::Scheduler,
};

#[test]
fn taskref_deallocates() {
    loom::model(|| {
        let track = Track::new(());
        let task = TaskRef::new(NopSchedule, async move {
            drop(track);
        });

        // if the task is not deallocated by dropping the `TaskRef`, the
        // `Track` will be leaked.
        drop(task);
    });
}

#[test]
fn taskref_clones_deallocate() {
    loom::model(|| {
        let track = Track::new(());
        let (task, _) = TaskRef::new(NopSchedule, async move {
            drop(track);
        });

        let mut threads = (0..2)
            .map(|_| {
                let task = task.clone();
                loom::thread::spawn(move || {
                    drop(task);
                })
            })
            .collect::<Vec<_>>();

        drop(task);

        for thread in threads.drain(..) {
            thread.join().unwrap();
        }
    });
}

#[test]
fn joinhandle_deallocates() {
    loom::model(|| {
        let track = Track::new(());
        let (task, join) = TaskRef::new(NopSchedule, async move {
            drop(track);
        });

        let thread = loom::thread::spawn(move || {
            drop(join);
        });

        drop(task);

        thread.join().unwrap();
    });
}

#[test]
fn join_handle_wakes() {
    loom::model(|| {
        let scheduler = Scheduler::new();
        let join = scheduler.spawn(TrackFuture::new(async move {
            future::yield_now().await;
            "hello world!"
        }));

        let scheduler_thread = loom::thread::spawn(move || {
            scheduler.tick();
        });

        let output = loom::future::block_on(join);
        assert_eq!(output.map(TrackFuture::into_inner), Ok("hello world!"));

        scheduler_thread.join().unwrap();
    })
}

#[test]
fn drop_join_handle() {
    loom::model(|| {
        let completed = Arc::new(AtomicBool::new(false));
        let scheduler = Scheduler::new();
        let join = scheduler.spawn({
            let completed = completed.clone();
            TrackFuture::new(async move {
                future::yield_now().await;
                completed.store(true, Ordering::Relaxed);
            })
        });

        let thread = loom::thread::spawn(move || {
            drop(join);
        });

        // tick the scheduler.
        scheduler.tick();

        thread.join().unwrap();
        assert!(completed.load(Ordering::Relaxed))
    })
}