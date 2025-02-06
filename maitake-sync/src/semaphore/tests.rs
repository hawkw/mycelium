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
    crate::loom::model(|| {
        // Semaphore with `DefaultRawMutex`
        let sem = Semaphore::new(1);
        assert_future(sem.acquire(1));

        // Semaphore with overridden `ScopedRawMutex`
        let sem = Semaphore::new_with_raw_mutex(1, NopRawMutex);
        assert_future(sem.acquire(1));
    });
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
        crate::loom::model(|| {
            // Semaphore with `DefaultRawMutex`
            let sem = alloc::sync::Arc::new(Semaphore::new(1));
            assert_future(sem.acquire_owned(1));

            // Semaphore with overridden `ScopedRawMutex`
            let sem = alloc::sync::Arc::new(Semaphore::new(1));
            assert_future(sem.acquire_owned(1));
        });
    }
}

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;

#[cfg(any(loom, feature = "alloc"))]
mod loom;
