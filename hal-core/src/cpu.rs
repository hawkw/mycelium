use alloc::boxed::Box;
use core::pin::Pin;

/// Abstraction for per-CPU local data storage mechanisms.
pub unsafe trait LocalData {
    fn get() -> Pin<&'static Self>;

    unsafe fn install(self: Pin<Box<Self>>);
}
