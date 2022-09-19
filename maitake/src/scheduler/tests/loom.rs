use super::*;
use crate::loom::{
    self,
    alloc::track_future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

#[test]
fn basically_works() {
    loom::model(|| {
        let scheduler = Scheduler::new();
        let it_worked = Arc::new(AtomicBool::new(false));

        scheduler.spawn({
            let it_worked = it_worked.clone();
            track_future(async move {
                future::yield_now().await;
                it_worked.store(true, Ordering::Release);
            })
        });

        let tick = scheduler.tick();

        assert!(it_worked.load(Ordering::Acquire));
        assert_eq!(tick.completed, 1);
        assert!(!tick.has_remaining);
        assert_eq!(tick.polled, 2)
    })
}

#[test]
fn notify_external() {
    loom::model(|| {
        let scheduler = Scheduler::new();
        let chan = Chan::new(1);
        let it_worked = Arc::new(AtomicBool::new(false));

        scheduler.spawn({
            let it_worked = it_worked.clone();
            let chan = chan.clone();
            track_future(async move {
                chan.wait().await;
                it_worked.store(true, Ordering::Release);
            })
        });

        thread::spawn(move || {
            chan.wake();
        });

        while scheduler.tick().completed < 1 {
            thread::yield_now();
        }

        assert!(it_worked.load(Ordering::Acquire));
    })
}

#[test]
fn notify_future() {
    loom::model(|| {
        let scheduler = Scheduler::new();
        let chan = Chan::new(1);
        let it_worked = Arc::new(AtomicBool::new(false));

        scheduler.spawn({
            let it_worked = it_worked.clone();
            let chan = chan.clone();
            track_future(async move {
                chan.wait().await;
                it_worked.store(true, Ordering::Release);
            })
        });

        scheduler.spawn(async move {
            future::yield_now().await;
            chan.wake();
        });

        test_dbg!(scheduler.tick());

        assert!(it_worked.load(Ordering::Acquire));
    })
}

#[test]
fn schedule_many() {
    const TASKS: usize = 10;
    loom::model(|| {
        let scheduler = Scheduler::new();
        let completed = Arc::new(AtomicUsize::new(0));

        for _ in 0..TASKS {
            scheduler.spawn({
                let completed = completed.clone();
                track_future(async move {
                    future::yield_now().await;
                    completed.fetch_add(1, Ordering::SeqCst);
                })
            });
        }

        let tick = scheduler.tick();

        assert_eq!(tick.completed, TASKS);
        assert_eq!(tick.polled, TASKS * 2);
        assert_eq!(completed.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    })
}

#[test]
fn current_task() {
    loom::model(|| {
        let scheduler = Scheduler::new();
        // drop the join handle, but keep the task ref.
        let task = {
            let join = scheduler.spawn({
                track_future(async move {
                    future::yield_now().await;
                })
            });
            join.task_ref()
        };

        let thread = loom::thread::spawn({
            let scheduler = scheduler.clone();
            move || {
                let current = scheduler.current_task();
                if let Some(current) = current {
                    assert_eq!(current, task);
                }
            }
        });

        loop {
            if scheduler.tick().completed > 0 {
                break;
            }
        }

        thread.join().expect("thread should not panic");
    })
}

#[test]
#[ignore] // this hits what i *believe* is a loom bug: https://github.com/tokio-rs/loom/issues/260
fn cross_thread_spawn() {
    const TASKS: usize = 10;
    loom::model(|| {
        let scheduler = Scheduler::new();
        let completed = Arc::new(AtomicUsize::new(0));
        let all_spawned = Arc::new(AtomicBool::new(false));
        loom::thread::spawn({
            let scheduler = scheduler.clone();
            let completed = completed.clone();
            let all_spawned = all_spawned.clone();
            move || {
                for _ in 0..TASKS {
                    scheduler.spawn({
                        let completed = completed.clone();
                        track_future(async move {
                            future::yield_now().await;
                            completed.fetch_add(1, Ordering::SeqCst);
                        })
                    });
                }
                all_spawned.store(true, Ordering::Release);
            }
        });

        let mut tick;
        loop {
            tick = scheduler.tick();
            if all_spawned.load(Ordering::Acquire) {
                break;
            }
            loom::thread::yield_now();
        }

        assert_eq!(completed.load(Ordering::SeqCst), TASKS);
        assert!(!tick.has_remaining);
    })
}

#[test]
fn injector() {
    const TASKS: usize = 10;
    const THREADS: usize = 3;
    loom::model(|| {
        let injector = Arc::new(steal::Injector::new());
        let completed = Arc::new(AtomicUsize::new(0));
        let all_spawned = Arc::new(AtomicBool::new(false));
        let threads = (1..=THREADS)
            .map(|worker| {
                let injector = injector.clone();
                let all_spawned = all_spawned.clone();
                let thread = thread::spawn(move || {
                    let scheduler = Scheduler::new();
                    info!(worker, "started");
                    loop {
                        let tick = scheduler.tick();
                        let stolen = injector
                            .try_steal()
                            .map(|stealer| stealer.spawn_n(&scheduler, usize::MAX))
                            .unwrap_or(0);
                        info!(worker, ?tick, ?stolen);
                        if !tick.has_remaining && stolen == 0 && all_spawned.load(Ordering::SeqCst)
                        {
                            break;
                        }
                        thread::yield_now();
                    }

                    info!(worker, "done");
                });
                info!(worker, "spawned worker thread");
                thread
            })
            .collect::<Vec<_>>();

        for _ in 0..TASKS {
            injector.spawn({
                let completed = completed.clone();
                track_future(async move {
                    future::yield_now().await;
                    completed.fetch_add(1, Ordering::SeqCst);
                })
            });
        }

        all_spawned.store(true, Ordering::SeqCst);

        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(TASKS, completed.load(Ordering::SeqCst));
    })
}
