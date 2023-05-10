use super::{Timer, TimerError};
use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

static GLOBAL_TIMER: AtomicPtr<Timer> = AtomicPtr::new(ptr::null_mut());

/// Errors returned by [`set_global_timer`].
#[derive(Debug)]
pub struct AlreadyInitialized(());

/// Sets a [`Timer`] as the [global default timer].
///
/// This function must be called in order for the [`sleep`] and [`timeout`] free
/// functions to be used.
///
/// The global timer can only be set a single time. Once the global timer is
/// initialized, subsequent calls to this function will return an
/// [`AlreadyInitialized`] error.
///
/// [`sleep`]: crate::time::sleep()
/// [`timeout`]: crate::time::timeout()
/// [global default timer]: crate::time#global-timers
pub fn set_global_timer(timer: &'static Timer) -> Result<(), AlreadyInitialized> {
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
pub(in crate::time) fn default() -> Result<&'static Timer, TimerError> {
    let ptr = GLOBAL_TIMER.load(Ordering::Acquire);
    ptr::NonNull::new(ptr)
        .ok_or(TimerError::NoGlobalTimer)
        .map(|ptr| unsafe {
            // safety: we have just null-checked this pointer, so we know it's not
            // null. and it's safe to convert it to an `&'static Timer`, because we
            // know that the pointer stored in the atomic *came* from an `&'static
            // Timer` (as it's only set in `set_global_timer`).
            ptr.as_ref()
        })
}
