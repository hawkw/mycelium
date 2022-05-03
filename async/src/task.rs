use crate::{loom::atomic::Ordering, scheduler::Schedule, util::non_null};
use alloc::boxed::Box;
use cordyceps::{mpsc_queue, Linked};

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

pub(crate) use self::state::State;
use self::state::StateVar;

#[repr(C)]
pub(crate) struct TaskRef {
    run_queue: mpsc_queue::Links<TaskRef>,
    // task_list: list::Links<TaskRef>,
    vtable: &'static Vtable,
    state: StateVar,
}

#[repr(C)]
#[pin_project]
pub(crate) struct Task<S, F: Future> {
    header: TaskRef,

    scheduler: S,
    #[pin]
    inner: Cell<F>,
}

#[pin_project(project = CellProj)]
enum Cell<F: Future> {
    Future(#[pin] F),
    Finished(F::Output),
}

struct Vtable {
    /// Poll the future.
    poll: unsafe fn(NonNull<TaskRef>) -> Poll<()>,
    /// Drops the task and deallocates its memory.
    drop_task: unsafe fn(NonNull<TaskRef>),
}

// === impl Task ===

impl<S: Schedule, F: Future> Task<S, F> {
    const TASK_VTABLE: Vtable = Vtable {
        poll: Self::poll,
        drop_task: Self::drop_task,
    };

    const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake_by_val,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    pub(crate) fn new(scheduler: S, future: F) -> NonNull<TaskRef> {
        let task_ptr = Box::leak(Box::new(Self {
            header: TaskRef {
                run_queue: mpsc_queue::Links::new(),
                vtable: &Self::TASK_VTABLE,
                state: StateVar::default(),
            },
            scheduler,
            inner: Cell::Future(future),
        }));
        NonNull::from(task_ptr).cast()
    }

    pub(crate) fn clone_ref(&self) {
        self.header.clone_ref();
    }

    pub(crate) fn drop_ref(&self) -> bool {
        if self.header.state.drop_ref() {
            // if `drop_ref` is called on the `Task` cell directly, elide
            // the vtable call.

            unsafe {
                Self::drop_task(NonNull::from(self).cast());
            }
            return true;
        }

        false
    }

    /// # Safety
    ///
    /// The `TaskRef` must point to a task with the same type parameters as `Self`.
    unsafe fn from_ref(ptr: NonNull<TaskRef>) -> NonNull<Self> {
        ptr.cast()
    }

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
        let task = non_null(ptr as *mut ()).cast::<Self>().as_ref();
        task.clone_ref();
        task.raw_waker()
    }

    unsafe fn drop_waker(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::drop_waker",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        non_null(ptr as *mut ()).cast::<Self>().as_ref().drop_ref();
    }

    unsafe fn wake_by_val(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::wake_by_val",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        let ptr = non_null(ptr as *mut ()).cast::<Self>();
        Self::schedule(ptr);
        ptr.as_ref().header.drop_ref();
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::wake_by_ref",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        Self::schedule(non_null(ptr as *mut ()).cast::<Self>())
    }

    #[inline(always)]
    unsafe fn schedule(this: NonNull<Self>) {
        this.as_ref().scheduler.schedule(this.cast::<TaskRef>());
    }

    unsafe fn poll(ptr: NonNull<TaskRef>) -> Poll<()> {
        let this = ptr.cast::<Self>().as_mut();
        let waker = Waker::from_raw(this.raw_waker());
        let cx = Context::from_waker(&waker);
        Pin::new_unchecked(this).poll_inner(cx)
    }

    unsafe fn drop_task(ptr: NonNull<TaskRef>) {
        drop(Box::from_raw(ptr.cast::<Self>().as_ptr()))
    }

    fn poll_inner(self: Pin<&mut Self>, mut cx: Context<'_>) -> Poll<()> {
        let mut this = self.project();
        let res = {
            let future = match this.inner.as_mut().project() {
                CellProj::Future(future) => future,
                CellProj::Finished(_) => unreachable!("tried to poll a completed future!"),
            };
            future.poll(&mut cx)
        };
        match res {
            Poll::Ready(ready) => {
                this.inner.set(Cell::Finished(ready));
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

impl TaskRef {
    pub(crate) fn state(&self) -> State {
        self.state.load(Ordering::Acquire)
    }

    pub(crate) fn clone_ref(&self) {
        self.state.clone_ref();
    }

    pub(crate) fn drop_ref(&self) -> bool {
        if self.state.drop_ref() {
            unsafe { (self.vtable.drop_task)(NonNull::from(self)) }
            return true;
        }

        false
    }

    pub(crate) fn poll(this: NonNull<Self>) -> Poll<()> {
        unsafe { (this.as_ref().vtable.poll)(this) }
    }
}
