// use core::task::{RawWaker, RawWakerVTable, Waker};

// unsafe fn clone_waker<T, S>(ptr: *const ()) -> RawWaker
// where
//     T: Future,
//     S: Schedule,
// {
//     let header = ptr as *const Header;
//     let ptr = NonNull::new_unchecked(ptr as *mut Header);
//     let harness = Harness::<T, S>::from_raw(ptr);
//     (*header).state.ref_inc();
//     raw_waker::<T, S>(ptr)
// }

// unsafe fn drop_waker<T, S>(ptr: *const ())
// where
//     T: Future,
//     S: Schedule,
// {
//     let ptr = NonNull::new_unchecked(ptr as *mut Header);
//     let harness = Harness::<T, S>::from_raw(ptr);
//     trace!(harness, "waker.drop");
//     harness.drop_reference();
// }

// unsafe fn wake_by_val<T, S>(ptr: *const ())
// where
//     T: Future,
//     S: Schedule,
// {
//     let ptr = NonNull::new_unchecked(ptr as *mut Header);
//     let harness = Harness::<T, S>::from_raw(ptr);
//     trace!(harness, "waker.wake");
//     harness.wake_by_val();
// }

// // Wake without consuming the waker
// unsafe fn wake_by_ref<T, S>(ptr: *const ())
// where
//     T: Future,
//     S: Schedule,
// {
//     let ptr = NonNull::new_unchecked(ptr as *mut Header);
//     let harness = Harness::<T, S>::from_raw(ptr);
//     trace!(harness, "waker.wake_by_ref");
//     harness.wake_by_ref();
// }

// fn raw_waker<T, S>(header: NonNull<Header>) -> RawWaker
// where
//     T: Future,
//     S: Schedule,
// {
//     let ptr = header.as_ptr() as *const ();
//     let vtable = &RawWakerVTable::new(
//         clone_waker::<T, S>,
//         wake_by_val::<T, S>,
//         wake_by_ref::<T, S>,
//         drop_waker::<T, S>,
//     );
//     RawWaker::new(ptr, vtable)
// }
