use super::Task;
use core::ptr::NonNull;
use core::future::Future;

pub trait HeapTask<S, F: Future> {
    fn into_raw(self) -> NonNull<Task<S, F>>;
    fn from_raw(ptr: NonNull<Task<S, F>>) -> Self;
}

feature! {
    #![feature = "alloc"]
    use alloc::boxed::Box;

    impl<S, F: Future> HeapTask<S, F> for Box<Task<S, F>> {
        fn into_raw(self) -> NonNull<Task<S, F>> {
            unsafe {
                crate::util::non_null(Box::into_raw(self))
            }
        }

        fn from_raw(ptr: NonNull<Task<S, F>>) -> Self {
            unsafe { Box::from_raw(ptr.as_ptr()) }
        }
    }

}
