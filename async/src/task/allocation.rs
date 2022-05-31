use super::Task;
use core::ptr::NonNull;
use core::future::Future;

pub trait Storage<S, F: Future>: Sized {
    type StoredTask;

    fn into_raw(task: Self::StoredTask) -> NonNull<Task<S, F, Self>>;
    fn from_raw(ptr: NonNull<Task<S, F, Self>>) -> Self::StoredTask;
}

feature! {
    #![feature = "alloc"]
    use alloc::boxed::Box;

    pub struct BoxStorage;

    impl<S, F: Future> Storage<S, F> for BoxStorage {
        type StoredTask = Box<Task<S, F, BoxStorage>>;

        fn into_raw(task: Box<Task<S, F, BoxStorage>>) -> NonNull<Task<S, F, BoxStorage>> {
            unsafe {
                crate::util::non_null(Box::into_raw(task))
            }
        }

        fn from_raw(ptr: NonNull<Task<S, F, BoxStorage>>) -> Box<Task<S, F, BoxStorage>> {
            unsafe { Box::from_raw(ptr.as_ptr()) }
        }
    }

}
