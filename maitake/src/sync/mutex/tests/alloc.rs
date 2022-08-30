use super::*;
use crate::scheduler::Scheduler;
use std::{future::Future, sync::Arc};

#[test]
fn basically_works() {
    const TASKS: usize = 10;

    let scheduler = Scheduler::new();
    let lock = Arc::new(Mutex::new(0));

    fn incr(lock: &Arc<Mutex<usize>>) -> impl Future + Send + 'static {
        let lock = lock.clone();
        async move {
            let mut guard = lock.lock().await;
            *guard += 1;
        }
    }

    for _ in 0..TASKS {
        scheduler.spawn(incr(&lock));
    }

    let mut completed = 0;
    while completed < TASKS {
        let tick = scheduler.tick();
        completed += tick.completed;
    }

    assert_eq!(*lock.try_lock().unwrap(), TASKS);
}

#[test]
#[cfg(feature = "alloc")]
fn lock_owned() {
    const TASKS: usize = 10;

    let scheduler = Scheduler::new();
    let lock = Arc::new(Mutex::new(0));

    fn incr(lock: &Arc<Mutex<usize>>) -> impl Future + Send + 'static {
        let lock = lock.clone().lock_owned();
        async move {
            let mut guard = lock.await;
            *guard += 1;
        }
    }

    for _ in 0..TASKS {
        scheduler.spawn(incr(&lock));
    }

    let mut completed = 0;
    while completed < TASKS {
        let tick = scheduler.tick();
        completed += tick.completed;
    }

    assert_eq!(*lock.try_lock().unwrap(), TASKS);
}
