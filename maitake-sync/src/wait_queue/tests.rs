use super::*;
use crate::util::test::{assert_future, NopRawMutex};

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;

#[cfg(any(loom, feature = "alloc"))]
mod loom;

#[test]
fn wait_future_is_future() {
    crate::loom::model(|| {
        // WaitQueue with default mutex.
        let q = WaitQueue::new();
        assert_future(q.wait());

        // WaitQueue with overridden `ScopedRawMutex`.
        let q = WaitQueue::new_with_raw_mutex(NopRawMutex);
        assert_future(q.wait());
    })
}
