use super::*;
use mycelium_util::sync::Lazy;

#[test]
fn basically_works() {
    static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static IT_WORKED: AtomicBool = AtomicBool::new(false);

    util::trace_init();

    SCHEDULER.spawn(async {
        future::yield_now().await;
        IT_WORKED.store(true, Ordering::Release);
    });

    let tick = SCHEDULER.tick();

    assert!(IT_WORKED.load(Ordering::Acquire));
    assert_eq!(tick.completed, 1);
    assert!(!tick.has_remaining);
    assert_eq!(tick.polled, 2)
}

#[test]
fn schedule_many() {
    static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    const TASKS: usize = 10;

    util::trace_init();
    for _ in 0..TASKS {
        SCHEDULER.spawn(async {
            future::yield_now().await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        });
    }

    let tick = SCHEDULER.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(tick.polled, TASKS * 2);
    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
    assert!(!tick.has_remaining);
}

#[test]
fn many_yields() {
    static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    const TASKS: usize = 10;

    util::trace_init();

    for i in 0..TASKS {
        SCHEDULER.spawn(async move {
            future::Yield::new(i).await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        });
    }

    let tick = SCHEDULER.tick();

    assert_eq!(tick.completed, TASKS);
    assert_eq!(COMPLETED.load(Ordering::SeqCst), TASKS);
    assert!(!tick.has_remaining);
}
