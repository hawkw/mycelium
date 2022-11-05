use super::Msr;
use alloc::boxed::Box;
use core::{arch::asm, marker::PhantomPinned, pin::Pin, ptr};
use hal_core::cpu::LocalData;

#[repr(C)]
#[derive(Debug)]
pub struct GsLocalData<T> {
    /// This *must* be the first field of the local data struct, because we read
    /// from `gs:0x0` to get the local data's address.
    _self: *const Self,
    /// Because this struct is self-referential, it may not be `Unpin`.
    _must_pin: PhantomPinned,
    /// Arbitrary user data.
    ///
    /// TODO(eliza): this needs to be replaced with something `LocalKey`-like
    /// --- currently you can only have one typed local data and we essentially
    /// do a stupid transmute to get it...
    userdata: T,
}

impl<T> GsLocalData<T> {
    pub const fn new(userdata: T) -> Self {
        Self {
            _self: ptr::null(),
            _must_pin: PhantomPinned,
            userdata,
        }
    }
}

unsafe impl<T> LocalData for GsLocalData<T> {
    fn get() -> Pin<&'static Self> {
        unsafe {
            let ptr: *const Self;
            asm!("mov {}, gs:0x0", out(reg) ptr);
            Pin::new_unchecked(&*ptr)
        }
    }

    unsafe fn install(self: Pin<Box<Self>>) {
        let this = Pin::into_inner_unchecked(self);
        let ptr = Box::into_raw(this);
        // set up self reference
        (*ptr)._self = ptr as *const _;
        Msr::ia32_gs_base().write(ptr as u64);
    }
}
