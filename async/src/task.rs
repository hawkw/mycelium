use crate::{
    loom::cell::UnsafeCell,
    scheduler::Schedule,
    util::{non_null, tracing},
};
use alloc::boxed::Box;
use cordyceps::{mpsc_queue, Linked};

pub use core::task::{Context, Poll, Waker};
use core::{
    any::type_name,
    fmt,
    future::Future,
    pin::Pin,
    ptr::NonNull,
    task::{RawWaker, RawWakerVTable},
};

mod state;

use self::state::StateVar;

#[derive(Debug)]
pub(crate) struct TaskRef(NonNull<Header>);

#[repr(C)]
#[derive(Debug)]
pub(crate) struct Header {
    run_queue: mpsc_queue::Links<Header>,
    state: StateVar,
    // task_list: list::Links<TaskRef>,
    vtable: &'static Vtable,
}

#[repr(C)]
struct Task<S, F: Future> {
    header: Header,

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
    poll: unsafe fn(NonNull<Header>) -> Poll<()>,
    /* // TODO(eliza): this will be needed when tasks can be dropped through `JoinHandle` refs...
    /// Drops the task and deallocates its memory.
    deallocate: unsafe fn(NonNull<Header>),
    */
}

// === impl Task ===

macro_rules! trace_task {
    ($ptr:expr, $f:ty, $method:literal) => {
        tracing::trace!(
            ptr = ?$ptr,
            concat!("Task::<Output = {}>::", $method),
            type_name::<<$f>::Output>()
        );
    };
}

impl<S: Schedule, F: Future> Task<S, F> {
    const TASK_VTABLE: Vtable = Vtable {
        poll: Self::poll,
        // deallocate: Self::deallocate,
    };

    const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake_by_val,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    fn allocate(scheduler: S, future: F) -> Box<Self> {
        Box::new(Self {
            header: Header {
                run_queue: mpsc_queue::Links::new(),
                vtable: &Self::TASK_VTABLE,
                state: StateVar::new(),
            },
            scheduler,
            inner: UnsafeCell::new(Cell::Future(future)),
        })
    }

    fn raw_waker(this: *const Self) -> RawWaker {
        unsafe { (*this).header.state.clone_ref() };
        RawWaker::new(this as *const (), &Self::WAKER_VTABLE)
    }

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        trace_task!(ptr, F, "clone_waker");

        Self::raw_waker(ptr as *const Self)
    }

    unsafe fn drop_waker(ptr: *const ()) {
        trace_task!(ptr, F, "drop_waker");

        let this = ptr as *const Self as *mut _;
        Self::drop_ref(non_null(this))
    }

    unsafe fn wake_by_val(ptr: *const ()) {
        trace_task!(ptr, F, "wake_by_val");

        let this = non_null(ptr as *mut ()).cast::<Self>();
        Self::schedule(this);
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        trace_task!(ptr, F, "wake_by_ref");

        Self::schedule(non_null(ptr as *mut ()).cast::<Self>())
    }

    #[inline(always)]
    unsafe fn schedule(this: NonNull<Self>) {
        this.as_ref()
            .scheduler
            .schedule(TaskRef(this.cast::<Header>()));
    }

    #[inline]
    unsafe fn drop_ref(this: NonNull<Self>) {
        trace_task!(this, F, "drop_ref");
        if !this.as_ref().header.state.drop_ref() {
            return;
        }

        drop(Box::from_raw(this.as_ptr()))
    }

    unsafe fn poll(ptr: NonNull<Header>) -> Poll<()> {
        trace_task!(ptr, F, "poll");
        let ptr = ptr.cast::<Self>();
        let waker = Waker::from_raw(Self::raw_waker(ptr.as_ptr()));
        let cx = Context::from_waker(&waker);
        let pin = Pin::new_unchecked(ptr.cast::<Self>().as_mut());
        let poll = pin.poll_inner(cx);
        if poll.is_ready() {
            Self::drop_ref(ptr);
        }

        poll
    }

    // unsafe fn deallocate(ptr: NonNull<Header>) {
    //     trace_task!(ptr, F, "deallocate");
    //     drop(Box::from_raw(ptr.cast::<Self>().as_ptr()))
    // }

    fn poll_inner(&self, mut cx: Context<'_>) -> Poll<()> {
        self.inner.with_mut(|cell| {
            let cell = unsafe { &mut *cell };
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
            .field("future_type", &type_name::<F>())
            .field("output_type", &type_name::<F::Output>())
            .field("scheduler_type", &type_name::<S>())
            .field("header", &self.header)
            .field("inner", &self.inner)
            .finish()
    }
}

// === impl TaskRef ===

impl TaskRef {
    pub(crate) fn new<S: Schedule, F: Future>(scheduler: S, future: F) -> Self {
        let task = Task::allocate(scheduler, future);
        let ptr = unsafe { non_null(Box::into_raw(task)).cast::<Header>() };
        tracing::trace!(
            ?ptr,
            "Task<..., Output = {}>::new",
            type_name::<F::Output>()
        );
        Self(ptr)
    }

    pub(crate) fn poll(&self) -> Poll<()> {
        let poll_fn = self.header().vtable.poll;
        unsafe { poll_fn(self.0) }
    }

    // #[inline]
    // fn state(&self) -> &StateVar {
    //     &self.header().state
    // }

    #[inline]
    fn header(&self) -> &Header {
        unsafe { self.0.as_ref() }
    }
}

// impl Clone for TaskRef {
//     #[inline]
//     fn clone(&self) -> Self {
//         self.state().clone_ref();
//         Self(self.0)
//     }
// }

// impl Drop for TaskRef {
//     #[inline]
//     fn drop(&mut self) {
//         if !self.state().drop_ref() {
//             return;
//         }

//         unsafe {
//             Header::drop_slow(self.0);
//         }
//     }
// }

unsafe impl Send for TaskRef {}
unsafe impl Sync for TaskRef {}

// === impl Header ===

/// # Safety
///
/// A task must be pinned to be spawned.
unsafe impl Linked<mpsc_queue::Links<Header>> for Header {
    type Handle = TaskRef;

    fn into_ptr(task: Self::Handle) -> NonNull<Self> {
        task.0
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
        TaskRef(ptr)
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

unsafe impl Send for Header {}
unsafe impl Sync for Header {}

impl<F: Future> fmt::Debug for Cell<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cell::Finished(_) => f.pad("Cell::Finished(...)"),
            Cell::Future(_) => f.pad("Cell::Future(...)"),
        }
    }
}
