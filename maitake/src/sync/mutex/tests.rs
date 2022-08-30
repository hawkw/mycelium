use super::*;

#[cfg(loom)]
mod loom;

// Tests which require `maitake`'s "alloc" feature flag.
#[cfg(all(not(loom), feature = "alloc"))]
mod alloc;

#[cfg(not(loom))]
#[test]
fn try_lock() {
    let mutex = Mutex::new(());

    let lock1 = mutex.try_lock();
    assert!(lock1.is_some());

    let lock2 = mutex.try_lock();
    assert!(lock2.is_none());
    drop(lock1);

    let lock3 = mutex.try_lock();
    assert!(lock3.is_some());
}
