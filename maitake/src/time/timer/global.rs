use super::Timer;
use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

static GLOBAL_TIMER: AtomicPtr<Timer> = AtomicPtr::new(ptr::null_mut());

#[derive(Debug)]
pub struct AlreadyInitialized(());

pub fn set_global_default(timer: &'static Timer) -> Result<(), AlreadyInitialized> {
    GLOBAL_TIMER
        .compare_exchange(
            ptr::null_mut(),
            timer as *const _ as *mut _,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|_| AlreadyInitialized(()))
        .map(|_| ())
}

#[inline(always)]
pub(in crate::time) fn default() -> &'static Timer {
    let ptr = GLOBAL_TIMER.load(Ordering::Acquire);
    assert_ne!(
        ptr,
        ptr::null_mut(),
        "a global timer first must be set by calling `timer::set_global_default`"
    );

    unsafe {
        // safety: we have just null-checked this pointer, so we know it's not
        // null. and it's safe to convert it to an `&'static Timer`, because we
        // know that the pointer stored in the atomic *came* from an `&'static
        // Timer` (as it's only set in `set_global_default`).
        &*ptr
    }
}
