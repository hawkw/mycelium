use crate::{
    loom::{
        cell::UnsafeCell,
        sync::{atomic::Ordering, Arc},
    },
    scheduler::Schedule,
    util::non_null,
};
use alloc::boxed::Box;
use cordyceps::{mpsc_queue, Linked};

pub use core::task::{Context, Poll, Waker};
use core::{
    any::type_name,
    fmt,
    future::Future,
    mem,
    pin::Pin,
    ptr::NonNull,
    task::{RawWaker, RawWakerVTable},
};
use pin_project::pin_project;

mod state;

// pub(crate) use self::state::State;
use self::state::StateVar;

#[repr(C)]
#[derive(Debug)]
pub(crate) struct TaskRef {
    run_queue: mpsc_queue::Links<TaskRef>,
    // task_list: list::Links<TaskRef>,
    vtable: &'static Vtable,
    // state: StateVar,
}

#[repr(C)]
pub(crate) struct Task<S, F: Future> {
    header: TaskRef,

    scheduler: S,
    inner: UnsafeCell<Cell<F>>,
}

enum Cell<F: Future> {
    Future(F),
    Finished(F::Output),
}

#[derive(Debug)]

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

    pub(crate) fn from_arc(arc: Arc<Self>) -> NonNull<TaskRef> {
        let ptr = Arc::into_raw(arc) as *mut Self as *mut TaskRef;
        unsafe { non_null(ptr) }
    }

    pub(crate) fn new_ref(scheduler: S, future: F) -> NonNull<TaskRef> {
        Self::from_arc(Arc::new(Self::new(scheduler, future)))
    }

    pub(crate) fn new(scheduler: S, future: F) -> Self {
        Self {
            header: TaskRef {
                run_queue: mpsc_queue::Links::new(),
                vtable: &Self::TASK_VTABLE,
                // state: StateVar::new(),
            },
            scheduler,
            inner: UnsafeCell::new(Cell::Future(future)),
        }
    }

    // pub(crate) fn clone_ref(&self) {
    //     self.header.clone_ref();
    // }

    // pub(crate) fn drop_ref(&self) -> bool {
    //     if test_dbg!(self.header.state.drop_ref()) {
    //         // if `drop_ref` is called on the `Task` cell directly, elide
    //         // the vtable call.

    //         unsafe {
    //             Self::drop_task(NonNull::from(self).cast());
    //         }
    //         return true;
    //     }

    //     false
    // }

    fn raw_waker(arc: &Arc<Self>) -> RawWaker {
        let ptr = Arc::into_raw(arc.clone()) as *const ();
        RawWaker::new(ptr, &Self::WAKER_VTABLE)
    }

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::clone_waker",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        let ptr = ptr as *const Self;
        let arc = mem::ManuallyDrop::new(Arc::from_raw(ptr));
        Self::raw_waker(&*arc)
    }

    unsafe fn drop_waker(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::drop_waker",
            type_name::<S>(),
            type_name::<F::Output>()
        );
        drop(Arc::from_raw(ptr as *const Self))
    }

    unsafe fn wake_by_val(ptr: *const ()) {
        tracing::trace!(
            ?ptr,
            "Task::<{}, dyn Future<Output = {}>::wake_by_val",
            type_name::<S>(),
            type_name::<F::Output>()
        );

        let task = non_null(ptr as *mut ()).cast::<Self>();
        Self::schedule(task);
        Arc::decrement_strong_count(ptr)
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
        let ptr = ptr.cast::<Self>().as_ptr() as *const _;
        let this = mem::ManuallyDrop::new(Arc::from_raw(ptr));
        let waker = Waker::from_raw(Self::raw_waker(&this));
        let cx = Context::from_waker(&waker);
        let poll = Pin::new(&this).poll_inner(cx);

        // Completing the final poll counts as consuming a ref --- if the task
        // has join interest, the `JoinHandle` will drop the final ref.
        // Otherwise, if the ref the task was polled through was the final ref,
        // it's safe to drop the task now.
        if test_dbg!(poll.is_ready()) {
            drop(mem::ManuallyDrop::into_inner(this));
        }

        poll
    }

    unsafe fn drop_task(ptr: NonNull<TaskRef>) {
        drop(Box::from_raw(ptr.cast::<Self>().as_ptr()))
    }

    fn poll_inner(&self, mut cx: Context<'_>) -> Poll<()> {
        test_println!("poll_inner: {:#?}", self);
        self.inner.with_mut(|cell| {
            let mut cell = unsafe { &mut *cell };
            let poll = match cell {
                Cell::Future(future) => unsafe { Pin::new_unchecked(future).poll(&mut cx) },
                Cell::Finished(_) => unreachable!("tried to poll a completed future!"),
            };

            match poll {
                Poll::Ready(ready) => {
                    *cell = Cell::Finished(ready);
                    Poll::Ready(())
                }
                Poll::Pending => Poll::Pending,
            }
        })
    }
}

unsafe impl<S: Send, F: Future + Send> Send for Task<S, F> {}
unsafe impl<S: Sync, F: Future + Sync> Sync for Task<S, F> {}

impl<S, F: Future> fmt::Debug for Task<S, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            .field("future_type", &core::any::type_name::<F>())
            .field("output_type", &core::any::type_name::<F::Output>())
            .field("scheduler_type", &core::any::type_name::<S>())
            .field("header", &self.header)
            .field("inner", &self.inner)
            .finish()
    }
}

// === impl TaskRef ===

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

unsafe impl Send for TaskRef {}
unsafe impl Sync for TaskRef {}

impl TaskRef {
    pub(crate) fn poll(this: NonNull<Self>) -> Poll<()> {
        unsafe { (this.as_ref().vtable.poll)(this) }
    }
}

impl<F: Future> fmt::Debug for Cell<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cell::Finished(_) => f.pad("Cell::Finished(...)"),
            Cell::Future(_) => f.pad("Cell::Future(...)"),
        }
    }
}
