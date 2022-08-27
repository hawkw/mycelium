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
}
