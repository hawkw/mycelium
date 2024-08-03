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
    // don't spawn as many tasks under miri, so that this test doesn't take forever...
    const TASKS: usize = if cfg!(miri) { 2 } else { 10 };

    // for some reason this branches slightly too many times for the default max
    // branches, IDK why...
    let mut model = loom::model::Builder::new();
    model.max_branches = 10_000;
    model.check(|| {
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
// this hits what i *believe* is a loom bug: https://github.com/tokio-rs/loom/issues/260
#[cfg_attr(loom, ignore)]
fn cross_thread_spawn() {
    // don't spawn as many tasks under miri, so that this test doesn't take forever...
    const TASKS: usize = if cfg!(miri) { 2 } else { 10 };
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
// this gets OOMkilled when running under loom, probably due to buffering too
// much tracing data across a huge number of iterations. skip it for now.
#[cfg_attr(loom, ignore)]
fn injector() {
    // when running in loom/miri, don't spawn all ten tasks, because that makes this
    // test run F O R E V E R
    const TASKS: usize = if cfg!(loom) || cfg!(miri) { 2 } else { 10 };
    const THREADS: usize = if cfg!(loom) || cfg!(miri) { 2 } else { 5 };
    let _trace = crate::util::trace_init();

    // for some reason this branches slightly too many times for the default max
    // branches, IDK why...
    let mut model = loom::model::Builder::new();
    model.max_branches = 10_000;
    model.check(|| {
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
                        let stolen = injector
                            .try_steal()
                            .map(|stealer| stealer.spawn_n(&scheduler, usize::MAX));
                        let tick = scheduler.tick();
                        info!(worker, ?tick, ?stolen);
                        if test_dbg!(!tick.has_remaining)
                            && test_dbg!(stolen == Err(TryStealError::Empty))
                            && test_dbg!(all_spawned.load(Ordering::SeqCst))
                        {
                            info!(worker, "finishing");
                            break;
                        }
                        info!(worker, "continuing");
                        thread::yield_now();
                    }

                    info!(worker, "done");
                });
                info!(worker, "spawned worker thread");
                thread
            })
            .collect::<Vec<_>>();

        for task in 0..TASKS {
            injector.spawn({
                let completed = completed.clone();
                track_future(async move {
                    info!(task, "started");
                    future::yield_now().await;
                    completed.fetch_add(1, Ordering::SeqCst);
                    info!(task, "completed");
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
