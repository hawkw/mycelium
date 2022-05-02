use crate::{loom::UnsafeCell, scheduler::Schedule};
use alloc::boxed::Box;
use cordyceps::mpsc_queue::{self, MpscQueue};

pub use core::task::{Context, Poll, Waker};
use core::{
    any::type_name,
    future::Future,
    pin::Pin,
    ptr::NonNull,
    task::{RawWaker, RawWakerVTable},
};
use core::{future::Future, pin::Pin, ptr::NonNull};

mod task_list;

pub(crate) struct TaskRef<S> {
    run_queue: mpsc_queue::Links<TaskRef<S>>,

    vtable: &'static Vtable,
    scheduler: S,
}

#[repr(C)]
pub(crate) struct Task<S, F> {
    header: TaskRef<S>,
    core: Core<F>,
}

enum Core<F: Future> {
    Future(F),
    Finished(F::Output),
}

struct Vtable {}

// === impl Task ===

/// # Safety
///
/// A task must be pinned to be spawned.
unsafe impl<S> list::Linked for Task<S> {
    type Handle = NonNull<Task<S>>;
    type Node = TaskRef<S>;

    fn as_ptr(task: &Self::Handle) -> NonNull<Self::Node> {
        unsafe {
            let task = task.as_ref();
            NonNull::from(&task.header)
        }
    }

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self::Node>) -> Self::Handle {
        NonNull::new_unchecked(ptr.as_ptr() as *mut Task<S>)
    }

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self::Node>) -> NonNull<list::Links<Self::Node>> {
        ptr.as_ref()
            .run_queue
            .with_mut(|links| NonNull::new_unchecked(links))
    }
}

// === impl Header ===

unsafe impl<S: Send> Send for TaskRef<S> {}
unsafe impl<S: Sync> Sync for TaskRef<S> {}

impl<S: Schedule> TaskRef<S> {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake_by_val,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    fn raw_waker(&self) -> RawWaker {
        RawWaker::new(self as *const _ as *const (), &Self::VTABLE)
    }

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        tracing::trace!(?ptr, "Header::<{}>::clone_waker", type_name::<S>());
        let this = &*(ptr as *const Self);
        this.scheduler.clone_ref();
        this.raw_waker()
    }

    unsafe fn drop_waker(ptr: *const ()) {
        tracing::trace!(?ptr, "Header::<{}>::drop_waker", type_name::<S>());
        let this = &*(ptr as *const Self);
        this.scheduler.drop_ref();
    }

    unsafe fn wake_by_val(ptr: *const ()) {
        tracing::trace!(?ptr, "Header::<{}>::wake_by_val", type_name::<S>());
        let ptr = ptr as *const Self;
        let scheduler = (*ptr).scheduler;
        this.scheduler
            .schedule(Pin::new(Box::from_raw(ptr as *mut _)));
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        tracing::trace!(?ptr, "Header::<{}>::wake_by_ref", type_name::<S>());
        let ptr = ptr as *const Self;
        let scheduler = (*ptr).scheduler;
        this.scheduler
            .schedule(Pin::new(Box::from_raw(ptr as *mut _)));
    }
}
