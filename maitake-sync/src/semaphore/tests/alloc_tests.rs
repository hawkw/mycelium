use super::*;
use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;

#[tokio::test]
async fn basic_concurrency_limit() {
    const TASKS: usize = 8;
    const CONCURRENCY_LIMIT: usize = 4;
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY_LIMIT));
    let running = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));

    for _ in 0..TASKS {
        let semaphore = semaphore.clone();
        let running = running.clone();
        let completed = completed.clone();
        tokio::spawn(async move {
            let permit = semaphore
                .acquire(1)
                .await
                .expect("semaphore will not be closed");
            assert!(test_dbg!(running.fetch_add(1, Relaxed)) < CONCURRENCY_LIMIT);

            tokio::task::yield_now().await;
            drop(permit);

            assert!(test_dbg!(running.fetch_sub(1, Relaxed)) <= CONCURRENCY_LIMIT);
            completed.fetch_add(1, Relaxed);
        });
    }

    while completed.load(Relaxed) < TASKS {
        assert!(test_dbg!(running.load(Relaxed)) <= CONCURRENCY_LIMIT);
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn countdown() {
    const TASKS: usize = 4;
    let _trace = crate::util::trace_init();
    let semaphore = Arc::new(Semaphore::new(0));
    let a_done = Arc::new(AtomicUsize::new(0));

    let b = tokio::spawn({
        let semaphore = semaphore.clone();
        let a_done = a_done.clone();
        async move {
            tracing::info!("Task B starting...");

            // Since the semaphore is created with 0 permits, this will
            // wait until all 4 "A" tasks have completed.
            let _permit = semaphore
                .acquire(TASKS)
                .await
                .expect("semaphore will not be closed");
            assert_eq!(a_done.load(Relaxed), TASKS);

            // ... do some work ...

            tracing::info!("Task B done!");
        }
    });

    for i in 0..TASKS {
        let semaphore = semaphore.clone();
        let a_done = a_done.clone();
        tokio::spawn(async move {
            tracing::info!("Task A {i} starting...");

            tokio::task::yield_now().await;

            a_done.fetch_add(1, Relaxed);
            semaphore.add_permits(1);

            // ... do some work ...
            tracing::info!("Task A {i} done");
        });
    }

    b.await.unwrap();
    assert_eq!(a_done.load(Relaxed), TASKS);
}
