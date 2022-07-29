#[cfg(loom)]
mod loom {
    use crate::{
        future,
        loom::{
            self,
            alloc::{Track, TrackFuture},
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
        },
        scheduler::Scheduler,
        task::*,
    };
    #[derive(Clone)]
    struct NopScheduler;

    impl crate::scheduler::Schedule for NopScheduler {
        fn schedule(&self, task: TaskRef) {
            unimplemented!(
                "nop scheduler should not actually schedule tasks (tried to schedule {task:?})"
            )
        }
    }

    #[test]
    fn taskref_deallocates() {
        loom::model(|| {
            let track = Track::new(());
            let task = TaskRef::new(NopScheduler, async move {
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
            let (task, _) = TaskRef::new(NopScheduler, async move {
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
            let (task, join) = TaskRef::new(NopScheduler, async move {
                drop(track);
            });

            let mut thread = loom::thread::spawn(move || {
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
}

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc {
    use crate::{
        future,
        scheduler::{Schedule, Scheduler},
        task::*,
    };
    use alloc::boxed::Box;
    use core::{
        ptr,
        sync::atomic::{AtomicBool, Ordering},
    };

    #[derive(Copy, Clone, Debug)]
    struct NopSchedule;

    impl Schedule for NopSchedule {
        fn schedule(&self, task: TaskRef) {
            unimplemented!("nop scheduler tried to schedule task {:?}", task);
        }
    }

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
                "header pointer and task allocation pointer must have the same address!"
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
        tokio_test::assert_pending!(test_dbg!(join.poll()), "join handle should be pending");
        assert!(!join.is_woken());

        // tick the scheduler.
        scheduler.tick();

        // the spawned task should complete on this tick.
        assert!(join.is_woken());
        let output =
            tokio_test::assert_ready_ok!(test_dbg!(join.poll()), "join handle should be notified");
        assert_eq!(test_dbg!(output), "hello world!");
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
