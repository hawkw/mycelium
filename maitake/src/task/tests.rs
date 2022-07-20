#[cfg(loom)]
mod loom {
    use crate::loom::{self, alloc::Track};
    use crate::task::*;

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
            let task = TaskRef::new(NopScheduler, async move {
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
}

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc {
    use crate::{scheduler::Schedule, task::*};
    use alloc::boxed::Box;
    use core::ptr;

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
        let header_ptr = unsafe { ptr::addr_of!((*task_ptr).header) };
        assert_eq!(
            task_ptr as *const (), header_ptr as *const (),
            "header pointer and task allocation pointer must have the same address!"
        );

        // clean up after ourselves by ensuring the box is deallocated
        unsafe { drop(Box::from_raw(task_ptr)) }
    }
}
