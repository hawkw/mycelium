#[cfg(loom)]
mod loom {
    use super::super::*;
    use crate::loom::{
        self, future,
        sync::{
            atomic::{AtomicUsize, Ordering::SeqCst},
            Arc,
        },
        thread,
    };

    #[test]
    fn basically_works() {
        const TASKS: usize = 2;

        async fn task((ref sem, ref count): &(Semaphore, AtomicUsize)) {
            let permit = sem.acquire(1).await.unwrap();
            let actual = count.fetch_add(1, SeqCst);
            assert!(actual <= TASKS - 1);

            let actual = count.fetch_sub(1, SeqCst);
            assert!(actual <= TASKS);
            drop(permit);
        }

        loom::model(|| {
            let sem = Arc::new((Semaphore::new(TASKS), AtomicUsize::new(0)));
            let threads = (0..TASKS)
                .map(|_| {
                    let sem = sem.clone();
                    thread::spawn(move || {
                        future::block_on(task(&sem));
                    })
                })
                .collect::<Vec<_>>();

            future::block_on(task(&sem));

            for t in threads {
                t.join().unwrap();
            }
        })
    }

    #[test]
    fn release_on_drop() {
        loom::model(|| {
            let sem = Arc::new(Semaphore::new(1));

            let thread = thread::spawn({
                let sem = sem.clone();
                move || {
                    let _permit = future::block_on(sem.acquire(1)).unwrap();
                }
            });

            let permit = future::block_on(sem.acquire(1)).unwrap();
            drop(permit);
            thread.join().unwrap();
        })
    }

    #[test]
    fn close() {
        loom::model(|| {
            let sem = Arc::new(Semaphore::new(1));
            let threads: Vec<_> = (0..2)
                .map(|_| {
                    thread::spawn({
                        let sem = sem.clone();
                        move || -> Result<(), ()> {
                            for _ in 0..2 {
                                let _permit = future::block_on(sem.acquire(1)).map_err(|_| ())?;
                            }
                            Ok(())
                        }
                    })
                })
                .collect();

            sem.close();

            for thread in threads {
                thread.join().unwrap();
            }
        })
    }

    #[test]
    fn concurrent_close() {
        fn run(sem: Arc<Semaphore>) -> impl FnOnce() -> Result<(), ()> {
            move || {
                let permit = future::block_on(sem.acquire(1)).map_err(|_| ())?;
                drop(permit);
                sem.close();
                Ok(())
            }
        }

        loom::model(|| {
            let sem = Arc::new(Semaphore::new(1));
            let threads: Vec<_> = (0..2).map(|_| thread::spawn(run(sem.clone()))).collect();
            let _ = run(sem)();

            for thread in threads {
                let _ = thread.join().unwrap();
            }
        })
    }
}
