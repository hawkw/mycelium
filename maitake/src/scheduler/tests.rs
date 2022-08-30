use super::*;
use crate::loom::sync::atomic::{AtomicUsize, Ordering};
use crate::{future, sync::wait_cell::test_util::Chan};

#[cfg(all(feature = "alloc", not(loom)))]
mod alloc {
    use super::*;
    use mycelium_util::sync::Lazy;

    #[test]
    fn notify_future() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        crate::util::trace_init();
        let chan = Chan::new(1);

        SCHEDULER.spawn({
            let chan = chan.clone();
            async move {
                chan.wait().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            }
        });

        SCHEDULER.spawn(async move {
            future::yield_now().await;
            chan.wake();
        });

        dbg!(SCHEDULER.tick());

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn notify_external() {
        static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        crate::util::trace_init();
        let chan = Chan::new(1);

        SCHEDULER.spawn({
            let chan = chan.clone();
            async move {
                chan.wait().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            }
        });

        std::thread::spawn(move || {
            chan.wake();
        });

        while dbg!(SCHEDULER.tick().completed) < 1 {
            std::thread::yield_now();
        }

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    }
}

#[cfg(not(loom))]
mod custom_storage {
    use super::*;
    use crate::task::{self, Task};
    use core::ptr::NonNull;

    /// A fake custom task allocation.
    ///
    /// This is actually just backed by `Box`, because we depend on `std` for
    /// tests, but it could be implemented with a custom allocator type.
    #[repr(transparent)]
    struct MyBoxTask<S, F: Future>(Box<Task<S, F, MyBoxStorage>>);

    struct MyBoxStorage;

    impl<S, F: Future> task::Storage<S, F> for MyBoxStorage {
        type StoredTask = MyBoxTask<S, F>;
        fn into_raw(MyBoxTask(task): Self::StoredTask) -> NonNull<Task<S, F, Self>> {
            NonNull::new(Box::into_raw(task)).expect("box must never be null!")
        }

        fn from_raw(ptr: NonNull<Task<S, F, Self>>) -> Self::StoredTask {
            unsafe { MyBoxTask(Box::from_raw(ptr.as_ptr())) }
        }
    }

    impl<F> MyBoxTask<&'static StaticScheduler, F>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        fn spawn(scheduler: &'static StaticScheduler, future: F) -> task::JoinHandle<F::Output> {
            let task = MyBoxTask(Box::new(Task::new(scheduler, future)));
            scheduler.spawn_allocated::<F, MyBoxStorage>(task)
        }
    }

    #[test]
    fn notify_future() {
        static STUB: TaskStub = TaskStub::new();
        static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        crate::util::trace_init();
        let chan = Chan::new(1);

        MyBoxTask::spawn(&SCHEDULER, {
            let chan = chan.clone();
            async move {
                chan.wait().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            }
        });

        MyBoxTask::spawn(&SCHEDULER, async move {
            future::yield_now().await;
            chan.wake();
        });

        dbg!(SCHEDULER.tick());

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn notify_external() {
        static STUB: TaskStub = TaskStub::new();
        static SCHEDULER: StaticScheduler = unsafe { StaticScheduler::new_with_static_stub(&STUB) };
        static COMPLETED: AtomicUsize = AtomicUsize::new(0);

        crate::util::trace_init();
        let chan = Chan::new(1);

        MyBoxTask::spawn(&SCHEDULER, {
            let chan = chan.clone();
            async move {
                chan.wait().await;
                COMPLETED.fetch_add(1, Ordering::SeqCst);
            }
        });

        std::thread::spawn(move || {
            chan.wake();
        });

        while dbg!(SCHEDULER.tick().completed) < 1 {
            std::thread::yield_now();
        }

        assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    }
}

#[cfg(loom)]
mod loom {
    use super::*;
    use crate::loom::{
        self,
        alloc::track_future,
        sync::{atomic::AtomicBool, Arc},
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
}
