use super::*;
use crate::util::test::{assert_future, NopRawMutex};

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;

#[cfg(any(loom, feature = "alloc"))]
mod loom;

#[test]
fn wait_future_is_future() {
    // WaitQueue with default raw mutex
    assert_future::<Wait<'_>>();
    // WaitQueue with overridden raw mutex
    assert_future::<Wait<'_, NopRawMutex>>();
}
