use super::*;
use crate::util;

#[test]
fn semaphore_is_send_and_sync() {
    util::test::assert_send_sync::<Semaphore>();
}

#[test]
fn permit_is_send_and_sync() {
    util::test::assert_send_sync::<Permit<'_>>();
}

#[test]
fn acquire_is_send_and_sync() {
    util::test::assert_send_sync::<crate::semaphore::Acquire<'_>>();
}

#[cfg(feature = "alloc")]
mod owned {
    use super::*;

    #[test]
    fn owned_permit_is_send_and_sync() {
        util::test::assert_send_sync::<OwnedPermit>();
    }

    #[test]
    fn acquire_owned_is_send_and_sync() {
        util::test::assert_send_sync::<AcquireOwned>();
    }
}

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc_tests;

#[cfg(any(loom, feature = "alloc"))]
mod loom;
