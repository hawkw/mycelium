#![cfg(feature = "alloc")]

use core::sync::atomic::{AtomicBool, Ordering};
use maitake::scheduler::Scheduler;

#[test]
fn basically_works() {
    let scheduler = Scheduler::new();
    static IT_WORKED: AtomicBool = AtomicBool::new(false);
    scheduler.build_task().name("hello world").spawn(async {
        println!("hello world");
        IT_WORKED.store(true, Ordering::Relaxed);
    });

    scheduler.tick();
    assert!(IT_WORKED.load(Ordering::Acquire));
}
