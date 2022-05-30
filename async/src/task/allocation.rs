use super::Task;
use core::ptr::NonNull;

pub trait HeapTask<S, F> {
    pub fn into_raw(self) -> NonNull<S, F>;
    pub fn from_raw(ptr: NonNull<S, F>) -> Self;
}

feature! {
    #![feature = "alloc"]
    use alloc::boxed::Box;

    impl<S, F> HeapTask<S, F> for Box<Task<S, F>> {
        pub fn into_raw(self) -> NonNull<S, F> {
            crate::util::non_null(Box::into_raw(self))
        }

        pub fn from_raw(ptr: NonNull<S, F>) -> Self {
            unsafe { Box::from_raw(ptr.as_ptr()) }
        }
    }

}
