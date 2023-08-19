use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use maitake::{future, scheduler::*};
use mycelium_util::sync::{Lazy, spin::Mutex};

#[path = "../util.rs"]
mod util;

#[cfg(feature = "alloc")]
mod alloc;
mod custom_storage;

#[test]
fn steal_blocked() {
    static SCHEDULER_1: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static SCHEDULER_2: Lazy<StaticScheduler> = Lazy::new(StaticScheduler::new);
    static MUTEX: Mutex<()> = Mutex::new(());
    static READY: AtomicBool = AtomicBool::new(false);
    static IT_WORKED: AtomicBool = AtomicBool::new(false);

    util::trace_init();

    let guard = MUTEX.lock();

    let thread = std::thread::spawn(|| {
        SCHEDULER_1.spawn(async {
            READY.store(true, Ordering::Release);

            // block this thread
            let _guard = MUTEX.lock();
        });

        SCHEDULER_1.spawn(async {
            IT_WORKED.store(true, Ordering::Release);
        });

        SCHEDULER_1.tick()
    });

    while !READY.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    assert!(SCHEDULER_1.current_task().is_some());

    let stolen = SCHEDULER_1.try_steal().unwrap().spawn_n(&SCHEDULER_2.get(), 1);
    assert_eq!(stolen, 1);

    let tick = SCHEDULER_2.tick();
    assert!(IT_WORKED.load(Ordering::Acquire));
    assert_eq!(tick.polled, 1);
    assert_eq!(tick.completed, 1);

    drop(guard);
    let tick = thread.join().unwrap();
    assert_eq!(tick.polled, 1);
    assert_eq!(tick.completed, 1);
}
