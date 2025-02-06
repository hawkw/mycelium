use super::*;
use crate::util::test::{assert_future, assert_send_sync, NopRawMutex};

#[test]
fn semaphore_is_send_and_sync() {
    assert_send_sync::<Semaphore>();
}

#[test]
fn permit_is_send_and_sync() {
    assert_send_sync::<Permit<'_>>();
}

#[test]
fn acquire_is_send_and_sync() {
    assert_send_sync::<crate::semaphore::Acquire<'_>>();
}

#[test]
fn acquire_is_future() {
    // Semaphore with `DefaultRawMutex`
    assert_future::<Acquire<'_>>();

    // Semaphore with overridden `ScopedRawMutex`
    assert_future::<Acquire<'_, NopRawMutex>>();
}

#[cfg(feature = "alloc")]
mod owned {
    use super::*;

    #[test]
    fn owned_permit_is_send_and_sync() {
        assert_send_sync::<OwnedPermit>();
    }

    #[test]
    fn acquire_owned_is_send_and_sync() {
        assert_send_sync::<AcquireOwned>();
    }

    #[test]
    fn acquire_owned_is_future() {
        assert_future::<AcquireOwned>();
        assert_future::<AcquireOwned<NopRawMutex>>();
    }
}

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;

#[cfg(any(loom, feature = "alloc"))]
mod loom;
