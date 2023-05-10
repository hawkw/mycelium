use super::*;
use crate::loom::sync::atomic::{AtomicUsize, Ordering};
use mycelium_util::sync::Lazy;

#[test]
fn notify_future() {
    static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let _trace = crate::util::trace_init();
    let chan = Chan::new(1);

    SCHEDULER.spawn({
        let chan = chan.clone();
        async move {
            chan.wait().await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        }
    });

    SCHEDULER.spawn(async move {
        future::yield_now().await;
        chan.wake();
    });

    dbg!(SCHEDULER.tick());

    assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
}

#[test]
fn notify_external() {
    static SCHEDULER: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static COMPLETED: AtomicUsize = AtomicUsize::new(0);

    let _trace = crate::util::trace_init();
    let chan = Chan::new(1);

    SCHEDULER.spawn({
        let chan = chan.clone();
        async move {
            chan.wait().await;
            COMPLETED.fetch_add(1, Ordering::SeqCst);
        }
    });

    std::thread::spawn(move || {
        chan.wake();
    });

    while dbg!(SCHEDULER.tick().completed) < 1 {
        std::thread::yield_now();
    }

    assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
}
