use super::*;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn semaphore_is_send_and_sync() {
    assert_send_sync::<Semaphore>();
}

#[test]
fn permit_is_send_and_sync() {
    assert_send_sync::<Permit<'_>>();
}

#[test]
fn acquire_is_send_and_sync() {
    assert_send_sync::<crate::wait::semaphore::Acquire<'_>>();
}

#[cfg(feature = "alloc")]
mod alloc {
    use super::*;
    use crate::scheduler::Scheduler;
    use ::alloc::sync::Arc;
    use core::sync::atomic::AtomicBool;

    #[test]
    fn owned_permit_is_send_and_sync() {
        assert_send_sync::<OwnedPermit>();
    }

    #[test]
    fn acquire_owned_is_send_and_sync() {
        assert_send_sync::<AcquireOwned>();
    }

    #[test]
    fn basic_concurrency_limit() {
        const TASKS: usize = 8;
        const CONCURRENCY_LIMIT: usize = 4;
        crate::util::trace_init();

        let scheduler = Scheduler::new();
        let semaphore = Arc::new(Semaphore::new(CONCURRENCY_LIMIT));
        let running = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));

        for _ in 0..TASKS {
            let semaphore = semaphore.clone();
            let running = running.clone();
            let completed = completed.clone();
            scheduler.spawn(async move {
                let permit = semaphore
                    .acquire(1)
                    .await
                    .expect("semaphore will not be closed");
                assert!(test_dbg!(running.fetch_add(1, Relaxed)) < CONCURRENCY_LIMIT);

                crate::future::yield_now().await;
                drop(permit);

                assert!(test_dbg!(running.fetch_sub(1, Relaxed)) <= CONCURRENCY_LIMIT);
                completed.fetch_add(1, Relaxed);
            });
        }

        while completed.load(Relaxed) < TASKS {
            scheduler.tick();
            assert!(test_dbg!(running.load(Relaxed)) <= CONCURRENCY_LIMIT);
        }
    }

    #[test]
    fn countdown() {
        const TASKS: usize = 4;

        let scheduler = Scheduler::new();
        let semaphore = Arc::new(Semaphore::new(0));
        let a_done = Arc::new(AtomicUsize::new(0));
        let b_done = Arc::new(AtomicBool::new(false));

        scheduler.spawn({
            let semaphore = semaphore.clone();
            let b_done = b_done.clone();
            let a_done = a_done.clone();
            async move {
                tracing_02::info!("Task B starting...");

                // Since the semaphore is created with 0 permits, this will
                // wait until all 4 "A" tasks have completed.
                let _permit = semaphore
                    .acquire(TASKS)
                    .await
                    .expect("semaphore will not be closed");
                assert_eq!(a_done.load(Relaxed), TASKS);

                // ... do some work ...

                tracing_02::info!("Task B done!");
                b_done.store(true, Relaxed);
            }
        });

        for i in 0..TASKS {
            let semaphore = semaphore.clone();
            let a_done = a_done.clone();
            scheduler.spawn(async move {
                tracing_02::info!("Task A {i} starting...");

                crate::future::yield_now().await;

                a_done.fetch_add(1, Relaxed);
                semaphore.add_permits(1);

                // ... do some work ...
                tracing_02::info!("Task A {i} done");
            });
        }

        while !b_done.load(Relaxed) {
            scheduler.tick();
        }

        assert_eq!(a_done.load(Relaxed), TASKS);
        assert!(b_done.load(Relaxed));
    }
}
