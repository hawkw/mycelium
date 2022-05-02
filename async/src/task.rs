use crate::scheduler::Schedule;
use alloc::boxed::Box;
use cordyceps::{list, mpsc_queue, Linked};

pub use core::task::{Context, Poll, Waker};
use core::{
    any::type_name,
    future::Future,
    pin::Pin,
    ptr::NonNull,
    task::{RawWaker, RawWakerVTable},
};
use pin_project::pin_project;

mod state;
mod task_list;
mod waker;
use self::state::State;
#[repr(C)]
pub(crate) struct TaskRef {
    run_queue: mpsc_queue::Links<TaskRef>,
    // task_list: list::Links<TaskRef>,
    vtable: &'static Vtable,
    state: State,
}

#[repr(C)]
#[pin_project]
pub(crate) struct Task<S, F: Future> {
    header: TaskRef,

    scheduler: S,
    #[pin]
    core: Core<F>,
}

#[pin_project(project = CoreProj)]
enum Core<F: Future> {
    Future(#[pin] F),
    Finished(F::Output),
}

struct Vtable {
    /// Poll the future.
    poll: unsafe fn(NonNull<TaskRef>),
    /// Drops the task and deallocates its memory.
    drop: unsafe fn(NonNull<TaskRef>),
}

// === impl Task ===

impl<S: Schedule, F: Future> Task<S, F> {
    fn vtable() -> &'static Vtable {
        todo!()
    }

    const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake_by_val,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    fn raw_waker(&self) -> RawWaker {
        RawWaker::new(self as *const _ as *const (), &Self::WAKER_VTABLE)
    }

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::clone_waker",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        let this = &*(ptr as *const Self);
        this.scheduler.clone_ref();
        this.raw_waker()
    }

    unsafe fn drop_waker(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::drop_waker",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        let this = &*(ptr as *const Self);
        this.scheduler.drop_ref();
    }

    unsafe fn wake_by_val(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::wake_by_val",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        let ptr = ptr as *const Self;
        let scheduler = &(*ptr).scheduler;
        scheduler.schedule(non_null(ptr as *mut _));
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::wake_by_ref",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        let ptr = ptr as *const Self;
        let scheduler = &(*ptr).scheduler;
        scheduler.schedule(non_null(ptr as *mut _));
    }

    unsafe fn poll(ptr: NonNull<TaskRef>) {
        // ptr.cast::<Task<S, F>().poll
        todo!()
    }

    fn poll_inner(self: Pin<&mut Self>, mut cx: Context<'_>) -> Poll<()> {
        let mut this = self.project();
        let res = {
            let future = match this.core.as_mut().project() {
                CoreProj::Future(future) => future,
                CoreProj::Finished(_) => unreachable!("tried to poll a completed future!"),
            };
            future.poll(&mut cx)
        };
        match res {
            Poll::Ready(ready) => {
                this.core.set(Core::Finished(ready));
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// # Safety
///
/// A task must be pinned to be spawned.
unsafe impl Linked<mpsc_queue::Links<TaskRef>> for TaskRef {
    type Handle = NonNull<Self>;

    fn into_ptr(task: Self::Handle) -> NonNull<Self> {
        task
    }

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<mpsc_queue::Links<Self>> {
        ptr.cast()
    }
}

// === impl TaskRef ===

unsafe impl Send for TaskRef {}
unsafe impl Sync for TaskRef {}

impl TaskRef {}

/// Helper to construct a `NonNull<T>` from a raw pointer to `T`, with null
/// checks elided in release mode.
#[cfg(debug_assertions)]
#[track_caller]
#[inline(always)]
unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new(ptr).expect(
        "/!\\ constructed a `NonNull` from a null pointer! /!\\ \n\
        in release mode, this would have called `NonNull::new_unchecked`, \
        violating the `NonNull` invariant! this is a bug in `cordyceps!`.",
    )
}
