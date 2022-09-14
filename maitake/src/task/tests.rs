use crate::{future, scheduler::Schedule, task::*};

#[derive(Copy, Clone, Debug)]
struct NopSchedule;

impl Schedule for NopSchedule {
    fn schedule(&self, task: TaskRef) {
        unimplemented!("nop scheduler tried to schedule task {:?}", task);
    }

    fn current_task(&self) -> Option<TaskRef> {
        unimplemented!("nop scheduler does not have a current task")
    }
}

#[cfg(any(loom, feature = "alloc"))]
mod loom {
    #![allow(clippy::drop_non_drop)]
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
    #[should_panic]
    fn do_leaks_work() {
        loom::model(|| {
            let track = Track::new(());
            std::mem::forget(track);
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
    fn join_handle_cancels_before_poll() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let join = scheduler.spawn(TrackFuture::new(async move {
                future::yield_now().await;
                "hello world!"
            }));

            assert!(join.cancel());
            let scheduler_thread = loom::thread::spawn(move || {
                scheduler.tick();
            });

            let err = loom::future::block_on(join).unwrap_err();
            assert!(err.is_canceled());

            scheduler_thread.join().unwrap();
        })
    }

    #[test]
    fn taskref_cancels() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let join = scheduler.spawn(TrackFuture::new(async move {
                future::yield_now().await;
                "hello world!"
            }));

            let taskref = join.task_ref();
            let cancel_thread = loom::thread::spawn(move || {
                assert!(taskref.cancel());
            });
            let scheduler_thread = loom::thread::spawn(move || {
                scheduler.tick();
            });

            scheduler_thread.join().unwrap();
            cancel_thread.join().unwrap();

            let err = loom::future::block_on(join).unwrap_err();
            assert!(err.is_canceled());
        })
    }

    #[test]
    fn join_handle_cancels_between_polls() {
        loom::model(|| {
            let scheduler = Scheduler::new();
            let join = scheduler.spawn(TrackFuture::new(async move {
                future::yield_now().await;
                future::yield_now().await;
                "hello world!"
            }));

            let scheduler_thread = loom::thread::spawn(move || {
                scheduler.tick();
                loom::thread::yield_now();
                scheduler.tick();
            });

            assert!(join.cancel());

            let err = loom::future::block_on(join).unwrap_err();
            assert!(err.is_canceled());

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
}

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests {
    use super::*;
    use crate::scheduler::Scheduler;
    use alloc::boxed::Box;
    use core::{
        ptr,
        sync::atomic::{AtomicBool, Ordering},
    };
    use tokio_test::{assert_pending, assert_ready_err, assert_ready_ok};

    /// This test ensures that layout-dependent casts in the `Task` struct's
    /// vtable methods are valid.
    #[test]
    fn task_is_valid_for_casts() {
        let task = Box::new(Task::<_, _, BoxStorage>::new(NopSchedule, async {
            unimplemented!("this task should never be polled")
        }));

        let task_ptr = Box::into_raw(task);

        // scope to ensure all task ptrs are dropped before we deallocate the
        // task allocation
        {
            let header_ptr = unsafe { ptr::addr_of!((*task_ptr).schedulable.header) };
            assert_eq!(
                task_ptr as *const (), header_ptr as *const (),
                "header pointer and task allocation pointer must have the same address!"
            );

            let sched_ptr = unsafe { ptr::addr_of!((*task_ptr).schedulable) };
            assert_eq!(
                task_ptr as *const (), sched_ptr as *const (),
                "schedulable pointer and task allocation pointer must have the same address!"
            );
        }

        // clean up after ourselves by ensuring the box is deallocated
        unsafe { drop(Box::from_raw(task_ptr)) }
    }

    /// This test just prints the size (in bytes) of an empty task struct.
    #[test]
    fn empty_task_size() {
        type Future = futures::future::Ready<()>;
        type EmptyTask = Task<NopSchedule, Future, BoxStorage>;
        println!(
            "{}: {}B",
            core::any::type_name::<EmptyTask>(),
            core::mem::size_of::<EmptyTask>(),
        );
        println!(
            "{}: {}B",
            core::any::type_name::<Future>(),
            core::mem::size_of::<Future>(),
        );
        println!(
            "task size: {}B",
            core::mem::size_of::<EmptyTask>() - core::mem::size_of::<Future>()
        );
    }

    #[test]
    fn join_handle_wakes() {
        crate::util::trace_init();

        let scheduler = Scheduler::new();
        let join = scheduler.spawn(async move {
            future::yield_now().await;
            "hello world!"
        });

        let mut join = tokio_test::task::spawn(join);

        // the join handle should be pending until the scheduler runs.
        assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
        assert!(!join.is_woken());

        // tick the scheduler.
        scheduler.tick();

        // the spawned task should complete on this tick.
        assert!(join.is_woken());
        let output = assert_ready_ok!(test_dbg!(join.poll()), "join handle should be notified");
        assert_eq!(test_dbg!(output), "hello world!");
    }

    #[test]
    fn join_handle_cancels_before_poll() {
        crate::util::trace_init();

        let scheduler = Scheduler::new();
        let join = scheduler.spawn(async move {
            future::yield_now().await;
            "hello world!"
        });

        let mut join = tokio_test::task::spawn(join);

        // the join handle should be pending until the scheduler runs.
        assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
        assert!(!join.is_woken());

        join.cancel();

        // tick the scheduler.
        scheduler.tick();

        // the spawned task should complete on this tick.
        assert!(join.is_woken());
        let err = assert_ready_err!(test_dbg!(join.poll()), "join handle should be notified");
        assert!(err.is_canceled(), "JoinError must be canceled");
        assert!(!err.is_completed(), "JoinError must be completed");
    }

    #[test]
    fn join_handle_cancels_after_poll() {
        crate::util::trace_init();

        let scheduler = Scheduler::new();
        let join = scheduler.spawn(async move {
            future::yield_now().await;
            "hello world!"
        });

        let mut join = tokio_test::task::spawn(join);

        // the join handle should be pending until the scheduler runs.
        assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
        assert!(!join.is_woken());

        // tick the scheduler.
        scheduler.tick();

        join.cancel();

        // the spawned task should complete on this tick.
        assert!(join.is_woken());
        let err = assert_ready_err!(test_dbg!(join.poll()), "join handle should be notified");
        assert!(err.is_canceled(), "JoinError must be canceled");
        assert!(err.is_completed(), "JoinError must be completed");
    }

    #[test]
    fn taskref_cancels_before_poll() {
        crate::util::trace_init();

        let scheduler = Scheduler::new();
        let join = scheduler.spawn(async move {
            future::yield_now().await;
            "hello world!"
        });

        let taskref = join.task_ref();
        let mut join = tokio_test::task::spawn(join);

        // the join handle should be pending until the scheduler runs.
        assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
        assert!(!join.is_woken());

        assert!(taskref.cancel());

        // tick the scheduler.
        scheduler.tick();

        // the spawned task should complete on this tick.
        assert!(join.is_woken());
        let err = assert_ready_err!(test_dbg!(join.poll()), "join handle should be notified");
        assert!(err.is_canceled(), "JoinError must be canceled");
        assert!(!err.is_completed(), "JoinError must be completed");
    }

    #[test]
    fn taskref_cancels_after_poll() {
        crate::util::trace_init();

        let scheduler = Scheduler::new();
        let join = scheduler.spawn(async move {
            future::yield_now().await;
            "hello world!"
        });

        let taskref = join.task_ref();
        let mut join = tokio_test::task::spawn(join);

        // the join handle should be pending until the scheduler runs.
        assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
        assert!(!join.is_woken());

        // tick the scheduler.
        scheduler.tick();

        assert!(taskref.cancel());

        // the spawned task should complete on this tick.
        assert!(join.is_woken());
        let err = assert_ready_err!(test_dbg!(join.poll()), "join handle should be notified");
        assert!(err.is_canceled(), "JoinError must be canceled");
        assert!(err.is_completed(), "JoinError must be completed");
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
}
