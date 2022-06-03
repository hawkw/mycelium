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
