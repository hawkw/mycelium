use super::*;
use crate::util::test::{assert_future, NopRawMutex};
#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;
#[cfg(any(loom, feature = "alloc"))]
mod loom;

#[test]
fn wait_future_is_future() {
    // WaitMap with default mutex.
    assert_future::<Wait<'_, i32, i32>>();

    // WaitMap with overridden `ScopedRawMutex`.
    assert_future::<Wait<'_, i32, i32, NopRawMutex>>();
}

#[test]
fn subscribe_future_is_future() {
    // WaitMap with default mutex.
    assert_future::<Subscribe<'_, '_, i32, i32>>();

    // WaitMap with overridden `ScopedRawMutex`.
    assert_future::<Subscribe<'_, '_, i32, i32, NopRawMutex>>();
}
