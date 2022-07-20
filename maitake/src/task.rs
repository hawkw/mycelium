//! The `maitake` task system.
//!
//! This module contains the code that spawns tasks on a [scheduler], and
//! manages the lifecycle of tasks once they are spawned. This includes the
//! in-memory representation of spawned tasks (the [`Task`] type), and the
//! handle used by the scheduler and other components of the runtime to
//! reference a task once it is spawned (the [`TaskRef`] type).
//!
//! [scheduler]: crate::scheduler
pub use self::builder::Builder;
#[cfg(feature = "alloc")]
pub use self::storage::BoxStorage;
pub use self::storage::Storage;
pub use core::task::{Context, Poll, Waker};

mod builder;
mod state;
mod storage;

#[cfg(test)]
mod tests;

use crate::{
    loom::cell::UnsafeCell,
    scheduler::Schedule,
    task::state::{OrDrop, ScheduleAction, StateCell},
    util::non_null,
};

use core::{
    any::type_name,
    future::Future,
    marker::PhantomData,
    mem,
    pin::Pin,
    ptr::{self, NonNull},
    task::{RawWaker, RawWakerVTable},
};

use self::builder::Settings;
use cordyceps::{mpsc_queue, Linked};
use mycelium_util::fmt;

/// A type-erased, reference-counted pointer to a spawned [`Task`].
///
/// Once a task has been spawned, it is generally referenced by a `TaskRef`.
/// When a spawned task is placed in a scheduler's run queue, dequeuing the next
/// task will yield a `TaskRef`, and a `TaskRef` may be converted into a
/// [`Waker`] or used to await a spawned task's completion.
///
/// `TaskRef`s are reference-counted, and the task will be deallocated when the
/// last `TaskRef` pointing to it is dropped.
#[derive(Debug)]
pub struct TaskRef(NonNull<Header>);

/// A task.
///
/// This type contains the various components of a task: the [future]
/// itself, the task's header, and a reference to the task's [scheduler]. When a
/// task is spawned, the `Task` type is placed on the heap (or wherever spawned
/// tasks are stored), and a type-erased [`TaskRef`] that points to that `Task`
/// is returned. Once a task is spawned, it is primarily interacted with via
/// [`TaskRef`]s.
///
/// [future]: core::future::Future
/// [scheduler]: crate::scheduler::Schedule
#[repr(C)]
pub struct Task<S, F: Future, STO> {
    /// The task's header.
    ///
    /// This contains the *untyped* components of the task which are identical
    /// regardless of the task's future, output, and scheduler types: the
    /// [vtable], [state cell], and [run queue links].
    ///
    /// # Safety
    ///
    /// This *must* be the first field in this type, to allow casting a
    /// `NonNull<Task>` to a `NonNull<Header>`.
    ///
    /// [vtable]: Vtable
    /// [state cell]: StateCell
    /// [run queue links]: cordyceps::mpsc_queue::Links
    header: Header,

    /// A reference to the [scheduler] this task is spawned on.
    ///
    /// This is used to schedule the task when it is woken.
    ///
    /// [scheduler]: crate::scheduler::Schedule
    scheduler: S,

    /// The task itself.
    ///
    /// This is either the task's [`Future`], when it is running,
    /// or the future's [`Output`], when the future has completed.
    ///
    /// [`Future`]: core::future::Future
    /// [`Output`]: core::future::Future::Output
    inner: UnsafeCell<Cell<F>>,

    span: crate::trace::Span,

    /// The [`Storage`] type associated with this struct
    ///
    /// In order to be agnostic over container types (e.g. [`Box`], or
    /// other user provided types), the Task is generic over a
    /// [`Storage`] type.
    ///
    /// [`Box`]: alloc::boxed::Box
    /// [`Storage`]: crate::task::Storage
    storage: PhantomData<STO>,
}

/// The task's header.
///
/// This contains the *untyped* components of the task which are identical
/// regardless of the task's future, output, and scheduler types: the
/// [vtable], [state cell], and [run queue links].
///
/// The header is the data at which a [`TaskRef`] points, and will likely be
/// prefetched when dereferencing a [`TaskRef`] pointer.[^1] Therefore, the
/// header should contain the task's most frequently accessed data, and should
/// ideally fit within a CPU cache line.
///
/// # Safety
///
/// The [run queue links] *must* be the first field in this type, in order for
/// the [`Linked::links` implementation] for this type to be sound. Therefore,
/// the `#[repr(C)]` attribute on this struct is load-bearing.
///
/// [vtable]: Vtable
/// [state cell]: StateCell
/// [run queue links]: cordyceps::mpsc_queue::Links
/// [`Linked::links` implementation]: #method.links
///
/// [^1]: On CPU architectures which support spatial prefetch, at least...
#[repr(C)]
#[derive(Debug)]
pub(crate) struct Header {
    /// The task's links in the intrusive run queue.
    ///
    /// # Safety
    ///
    /// This MUST be the first field in this struct.
    run_queue: mpsc_queue::Links<Header>,

    /// The task's state, which can be atomically updated.
    state: StateCell,

    /// The task vtable for this task.
    ///
    /// Note that this is different from the [waker vtable], which contains
    /// pointers to the waker methods (and depends primarily on the task's
    /// scheduler type). The task vtable instead contains methods for
    /// interacting with the task's future, such as polling it and reading the
    /// task's output. These depend primarily on the type of the future rather
    /// than the scheduler.
    ///
    /// [waker vtable]: core::task::RawWakerVTable
    vtable: &'static Vtable,
}

enum Cell<F: Future> {
    Future(F),
    Finished(F::Output),
}

#[derive(Debug)]
struct Vtable {
    /// Poll the future.
    poll: unsafe fn(TaskRef) -> Poll<()>,
    /// Drops the task and deallocates its memory.
    deallocate: unsafe fn(NonNull<Header>),
}

// === impl Task ===

macro_rules! trace_waker_op {
    ($ptr:expr, $f:ty, $method: ident) => {
        trace_waker_op!($ptr, $f, $method, op: $method)
    };
    ($ptr:expr, $f:ty, $method: ident, op: $op:ident) => {

        #[cfg(any(feature = "tracing-01", loom))]
        tracing_01::trace!(
            target: "runtime::waker",
            {
                task.id = (*$ptr).span.tracing_01_id(),
                task.addr = ?$ptr,
                task.output = %type_name::<<$f>::Output>(),
                op = concat!("waker.", stringify!($op)),
            },
            concat!("Task::", stringify!($method)),
        );


        #[cfg(not(any(feature = "tracing-01", loom)))]
        trace!(
            target: "runtime::waker",
            {
                task.addr = ?$ptr,
                task.output = %type_name::<<$f>::Output>(),
                op = concat!("waker.", stringify!($op)),
            },
            concat!("Task::", stringify!($method)),

        );
    };
}

impl<S, F, STO> Task<S, F, STO>
where
    S: Schedule,
    F: Future,
    STO: Storage<S, F>,
{
    const TASK_VTABLE: Vtable = Vtable {
        poll: Self::poll,
        deallocate: Self::deallocate,
    };

    const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake_by_val,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    /// Create a new (non-heap-allocated) Task.
    ///
    /// This needs to be heap allocated using an implementor of
    /// the [`Storage`] trait to be used with the scheduler.
    ///
    /// [`Storage`]: crate::task::Storage
    pub fn new(scheduler: S, future: F) -> Self {
        Self {
            header: Header {
                run_queue: mpsc_queue::Links::new(),
                vtable: &Self::TASK_VTABLE,
                state: StateCell::new(),
            },
            scheduler,
            inner: UnsafeCell::new(Cell::Future(future)),
            storage: PhantomData,
            span: crate::trace::Span::none(),
        }
    }

    fn raw_waker(this: *const Self) -> RawWaker {
        RawWaker::new(this as *const (), &Self::WAKER_VTABLE)
    }

    #[inline]
    fn state(&self) -> &StateCell {
        &self.header.state
    }

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        let this = ptr as *const Self;
        trace_waker_op!(this, F, clone_waker, op: clone);
        (*this).state().clone_ref();
        Self::raw_waker(this)
    }

    unsafe fn drop_waker(ptr: *const ()) {
        let ptr = ptr as *const Self;
        trace_waker_op!(ptr, F, drop_waker, op: drop);

        let this = ptr as *mut _;
        Self::drop_ref(non_null(this))
    }

    unsafe fn wake_by_val(ptr: *const ()) {
        let ptr = ptr as *const Self;
        trace_waker_op!(ptr, F, wake_by_val, op: wake);

        let this = non_null(ptr as *mut Self);
        match test_dbg!(this.as_ref().state().wake_by_val()) {
            OrDrop::Drop => drop(STO::from_raw(this)),
            OrDrop::Action(ScheduleAction::Enqueue) => {
                // the task should be enqueued.
                //
                // in the case that the task is enqueued, the state
                // transition does *not* decrement the reference count. this is
                // in order to avoid dropping the task while it is being
                // scheduled. one reference is consumed by enqueuing the task...
                Self::schedule(TaskRef(this.cast::<Header>()));
                // now that the task has been enqueued, decrement the reference
                // count to drop the waker that performed the `wake_by_val`.
                Self::drop_ref(this);
            }
            OrDrop::Action(ScheduleAction::None) => {}
        }
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        let ptr = ptr as *const Self;
        trace_waker_op!(ptr, F, wake_by_ref);

        let this = non_null(ptr as *mut ()).cast::<Self>();
        if this.as_ref().state().wake_by_ref() == ScheduleAction::Enqueue {
            Self::schedule(TaskRef(this.cast::<Header>()));
        }
    }

    #[inline(always)]
    unsafe fn schedule(this: TaskRef) {
        this.0.cast::<Self>().as_ref().scheduler.schedule(this);
    }

    #[inline]
    unsafe fn drop_ref(this: NonNull<Self>) {
        trace!(
            task.addr = ?this,
            task.output = %type_name::<<F>::Output>(),
            "Task::drop_ref"
        );
        if !this.as_ref().state().drop_ref() {
            return;
        }

        drop(STO::from_raw(this))
    }

    unsafe fn poll(ptr: TaskRef) -> Poll<()> {
        trace!(
            task.addr = ?ptr,
            task.output = %type_name::<<F>::Output>(),
            "Task::poll"
        );
        let mut this = ptr.0.cast::<Self>();
        test_trace!(task = ?fmt::alt(this.as_ref()));
        // try to transition the task to the polling state
        let state = &this.as_ref().state();
        match test_dbg!(state.start_poll()) {
            // transitioned successfully!
            Ok(_) => {}
            Err(_state) => {
                // TODO(eliza): could run the dealloc glue here instead of going
                // through a ref cycle?
                return Poll::Ready(());
            }
        }

        // wrap the waker in `ManuallyDrop` because we're converting it from an
        // existing task ref, rather than incrementing the task ref count. if
        // this waker is consumed during the poll, we don't want to decrement
        // its ref count when the poll ends.
        let waker = mem::ManuallyDrop::new(Waker::from_raw(Self::raw_waker(this.as_ptr())));
        let cx = Context::from_waker(&waker);

        // actually poll the task
        let pin = Pin::new_unchecked(this.as_mut());
        let poll = pin.poll_inner(cx);

        // post-poll state transition
        match test_dbg!(state.end_poll(poll.is_ready())) {
            OrDrop::Drop => drop(STO::from_raw(this)),
            OrDrop::Action(ScheduleAction::Enqueue) => Self::schedule(ptr),
            OrDrop::Action(ScheduleAction::None) => {}
        }

        poll
    }

    unsafe fn deallocate(ptr: NonNull<Header>) {
        trace!(
            task.addr = ?ptr,
            task.output = %type_name::<<F>::Output>(),
            "Task::deallocate"
        );
        let this = ptr.cast::<Self>();
        drop(STO::from_raw(this));
    }

    fn poll_inner(&self, mut cx: Context<'_>) -> Poll<()> {
        #[cfg(any(feature = "tracing-01", feature = "tracing-02", test))]
        let _span = self.span.enter();

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

unsafe impl<S, F, STO> Send for Task<S, F, STO>
where
    S: Send,
    F: Future + Send,
{
}
unsafe impl<S, F, STO> Sync for Task<S, F, STO>
where
    S: Sync,
    F: Future + Sync,
{
}

impl<S, F, STO> fmt::Debug for Task<S, F, STO>
where
    F: Future,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            // .field("future_type", &fmt::display(type_name::<F>()))
            .field("storage", &fmt::display(type_name::<STO>()))
            .field("output_type", &fmt::display(type_name::<F::Output>()))
            .field("scheduler_type", &fmt::display(type_name::<S>()))
            .field("header", &self.header)
            .field("inner", &self.inner)
            .finish()
    }
}

// === impl TaskRef ===

impl TaskRef {
    const NO_BUILDER: &'static Settings<'static> = &Settings::new();

    #[track_caller]
    pub(crate) fn new_allocated<S, F, STO>(task: STO::StoredTask) -> Self
    where
        S: Schedule,
        F: Future,
        STO: Storage<S, F>,
    {
        Self::build_allocated::<S, F, STO>(Self::NO_BUILDER, task)
    }

    #[track_caller]
    pub(crate) fn build_allocated<S, F, STO>(builder: &Settings<'_>, task: STO::StoredTask) -> Self
    where
        S: Schedule,
        F: Future,
        STO: Storage<S, F>,
    {
        #[allow(unused_mut)]
        let mut ptr = STO::into_raw(task);

        // attach the task span, if tracing is enabled.
        #[cfg(any(feature = "tracing-01", feature = "tracing-02", test))]
        {
            let loc = builder
                .location
                .as_ref()
                .unwrap_or_else(|| &*core::panic::Location::caller());
            let span = trace_span!(
                "runtime.spawn",
                kind = %builder.kind,
                // XXX(eliza): would be nice to not use emptystring here but
                // `tracing` 0.2 is missing `Option` value support :(
                task.name = builder.name.unwrap_or(""),
                task.addr = ?ptr,
                task.output = %type_name::<F::Output>(),
                task.storage = %type_name::<STO>(),
                loc.file = loc.file(),
                loc.line = loc.line(),
                loc.col = loc.column(),
            );
            unsafe {
                ptr.as_mut().span = span;
            };
        }

        let ptr = ptr.cast::<Header>();
        trace!(
            task.name = builder.name.unwrap_or(""),
            task.addr = ?ptr,
            task.kind = %builder.kind,
            "Task<..., Output = {}>::new",
            type_name::<F::Output>()
        );

        #[cfg(not(any(feature = "tracing-01", feature = "tracing-02", test)))]
        let _ = builder;
        Self(ptr)
    }

    pub(crate) fn poll(self) -> Poll<()> {
        let poll_fn = self.header().vtable.poll;
        unsafe { poll_fn(self) }
    }

    #[inline]
    fn state(&self) -> &StateCell {
        &self.header().state
    }

    #[inline]
    fn header(&self) -> &Header {
        unsafe { self.0.as_ref() }
    }
}

impl Clone for TaskRef {
    #[inline]
    fn clone(&self) -> Self {
        self.state().clone_ref();
        Self(self.0)
    }
}

impl Drop for TaskRef {
    #[inline]
    fn drop(&mut self) {
        if !self.state().drop_ref() {
            return;
        }

        unsafe {
            Header::deallocate(self.0);
        }
    }
}

unsafe impl Send for TaskRef {}
unsafe impl Sync for TaskRef {}

// === impl Header ===

// See https://github.com/rust-lang/rust/issues/97708 for why
// this is necessary
#[no_mangle]
unsafe fn _maitake_header_nop(_ptr: TaskRef) -> Poll<()> {
    #[cfg(debug_assertions)]
    unreachable!("stub task ({_ptr:?}) should never be polled!");
    #[cfg(not(debug_assertions))]
    Poll::Pending
}

// See https://github.com/rust-lang/rust/issues/97708 for why
// this is necessary
#[no_mangle]
unsafe fn _maitake_header_nop_deallocate(ptr: NonNull<Header>) {
    unreachable!("stub task ({ptr:p}) should never be deallocated!");
}

impl Header {
    #[cfg(not(loom))]
    pub(crate) const fn new_stub() -> Self {
        Self {
            run_queue: mpsc_queue::Links::new_stub(),
            state: StateCell::new(),
            vtable: &Vtable {
                poll: _maitake_header_nop,
                deallocate: _maitake_header_nop_deallocate,
            },
        }
    }

    unsafe fn deallocate(this: NonNull<Self>) {
        #[cfg(debug_assertions)]
        {
            let refs = this
                .as_ref()
                .state
                .load(core::sync::atomic::Ordering::Acquire)
                .ref_count();
            debug_assert_eq!(refs, 0, "tried to deallocate a task with references!");
        }

        let deallocate = this.as_ref().vtable.deallocate;
        deallocate(this)
    }
}

/// # Safety
///
/// A task must be pinned to be spawned.
unsafe impl Linked<mpsc_queue::Links<Header>> for Header {
    type Handle = TaskRef;

    #[inline]
    fn into_ptr(task: Self::Handle) -> NonNull<Self> {
        let ptr = task.0;
        // converting a `TaskRef` into a pointer to enqueue it assigns ownership
        // of the ref count to the queue, so we don't want to run its `Drop`
        // impl.
        mem::forget(task);
        ptr
    }

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    #[inline]
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
    #[inline]
    unsafe fn links(target: NonNull<Self>) -> NonNull<mpsc_queue::Links<Self>> {
        let target = target.as_ptr();
        // Safety: using `ptr::addr_of_mut!` avoids creating a temporary
        // reference, which stacked borrows dislikes.
        let links = ptr::addr_of_mut!((*target).run_queue);
        // Safety: it's fine to use `new_unchecked` here; if the pointer that we
        // offset to the `links` field is not null (which it shouldn't be, as we
        // received it as a `NonNull`), the offset pointer should therefore also
        // not be null.
        NonNull::new_unchecked(links)
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

// Additional types and capabilities only available with the "alloc"
// feature active
feature! {
    #![feature = "alloc"]

    use alloc::boxed::Box;

    impl TaskRef {

        #[track_caller]
        pub(crate) fn new<S, F>(scheduler: S, future: F) -> Self
        where
            S: Schedule,
            F: Future + 'static
        {
            let task = Box::new(Task::<S, F, BoxStorage>::new(scheduler, future));
            Self::build_allocated::<S, F, BoxStorage>(Self::NO_BUILDER, task)
        }
    }

}
