use super::*;
use crate::util::test::{assert_future, assert_send_sync, NopRawMutex};

#[cfg(any(loom, feature = "alloc"))]
mod loom;

#[cfg(not(loom))]
mod sequential;

#[test]
fn lock_is_send_sync() {
    assert_send_sync::<RwLock<usize>>();
}

#[test]
fn read_guard_is_send_sync() {
    assert_send_sync::<RwLockReadGuard<'_, usize>>();
}

#[test]
fn write_guard_is_send_sync() {
    assert_send_sync::<RwLockWriteGuard<'_, usize>>();
}

#[test]
fn read_future_is_future() {
    crate::loom::model(|| {
        // RwLock with default mutex.
        let q = RwLock::new(1);
        assert_future(q.read());

        // RwLock with overridden `ScopedRawMutex`.
        let q = RwLock::new_with_raw_mutex(1, NopRawMutex);
        assert_future(q.read());
    })
}

#[test]
fn write_future_is_future() {
    crate::loom::model(|| {
        // RwLock with default mutex.
        let q = RwLock::new(1);
        assert_future(q.write());

        // RwLock with overridden `ScopedRawMutex`.
        let q = RwLock::new_with_raw_mutex(1, NopRawMutex);
        assert_future(q.write());
    })
}
