use super::*;
use crate::util::test::{assert_future, NopRawMutex};
#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;
#[cfg(any(loom, feature = "alloc"))]
mod loom;

#[test]
fn wait_future_is_future() {
    crate::loom::model(|| {
        // WaitMap with default mutex.
        let m = WaitMap::<i32, i32>::new();
        assert_future(m.wait(1));

        // WaitMap with overridden `ScopedRawMutex`.
        let m = WaitMap::<i32, i32, _>::new_with_raw_mutex(NopRawMutex);
        assert_future(m.wait(1));
    })
}

#[test]
fn subscribe_future_is_future() {
    crate::loom::model(|| {
        // WaitMap with default mutex.
        let m = WaitMap::<i32, i32>::new();
        let f = m.wait(1);
        let f = core::pin::pin!(f);
        assert_future(f.subscribe());

        // WaitMap with overridden `ScopedRawMutex`.
        let m = WaitMap::<i32, i32, _>::new_with_raw_mutex(NopRawMutex);
        let f = m.wait(1);
        let f = core::pin::pin!(f);
        assert_future(f.subscribe());
    })
}
