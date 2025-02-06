use super::*;
use crate::util::test::assert_send_sync;

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
