//! The `maitake` task system.
//!
//! This module contains the code that spawns tasks on a [scheduler], and
//! manages the lifecycle of tasks once they are spawned. This includes the
//! in-memory representation of spawned tasks (the [`Task`] type), and the
//! handle used by the scheduler and other components of the runtime to
//! reference a task once it is spawned (the [`TaskRef`] type).
//!
//! [scheduler]: crate::scheduler
#[cfg(feature = "alloc")]
pub use self::storage::BoxStorage;
pub use self::{
    builder::Builder,
    join_handle::{JoinError, JoinHandle},
    storage::Storage,
};
pub use core::task::{Context, Poll, Waker};

mod builder;
mod join_handle;
mod state;
mod storage;

#[cfg(test)]
mod tests;

use crate::{loom::cell::UnsafeCell, scheduler::Schedule, trace, util::non_null};

use core::{
    any::type_name,
    future::Future,
    marker::PhantomData,
    mem,
    pin::Pin,
    ptr::{self, NonNull},
    task::{RawWaker, RawWakerVTable},
};

use self::{
    builder::Settings,
    state::{JoinAction, OrDrop, ScheduleAction, StateCell},
};
use cordyceps::{mpsc_queue, Linked};
use mycelium_util::{fmt, mem::CheckedMaybeUninit};

/// A type-erased, reference-counted pointer to a spawned [`Task`].
///
/// Once a task has been spawned, it is generally referenced by a `TaskRef`.
/// When a spawned task is placed in a scheduler's run queue, dequeuing the next
/// task will yield a `TaskRef`, and a `TaskRef` may be converted into a
/// [`Waker`] or used to await a spawned task's completion.
///
/// `TaskRef`s are reference-counted, and the task will be deallocated when the
/// last `TaskRef` pointing to it is dropped.
#[derive(Debug, Eq, PartialEq)]
pub struct TaskRef(NonNull<Header>);

/// A task.
///
/// This type contains the various components of a task: the [future][`Future`]
/// itself, the task's header, and a reference to the task's [scheduler]. When a
/// task is spawned, the `Task` type is placed on the heap (or wherever spawned
/// tasks are stored), and a type-erased [`TaskRef`] that points to that `Task`
/// is returned. Once a task is spawned, it is primarily interacted with via
/// [`TaskRef`]s.
///
/// ## Vtables and Type Erasure
///
/// The `Task` struct, once spawned, is rarely interacted with directly. Because
/// a system may spawn any number of different [`Future`] types as tasks, and
/// may potentially also contain multiple types of [scheduler] and/or [task
/// storage], the scheduler and other parts of the system generally interact
/// with tasks via type-erased [`TaskRef`]s.
///
/// However, in order to actually poll a task's [`Future`], or perform other
/// operations such as deallocating a task, it is necessary to know the type of
/// the the task's [`Future`] (and potentially, that of the scheduler and/or
/// storage). Therefore, operations that are specific to the task's `S`-typed
/// [scheduler], `F`-typed [`Future`], and `STO`-typed [`Storage`] are performed
/// via [dynamic dispatch].
///
/// [scheduler]: crate::scheduler::Schedule
/// [task storage]: Storage
/// [dynamic dispatch]: https://en.wikipedia.org/wiki/Dynamic_dispatch
#[repr(C)]
pub struct Task<S, F: Future, STO> {
    /// The task's [`Header`] and [scheduler].
    ///
    /// # Safety
    ///
    /// This must be the first field of the `Task` struct!
    ///
    /// [scheduler]: crate::scheduler::Schedule
    schedulable: Schedulable<S>,

    /// The task itself.
    ///
    /// This is either the task's [`Future`], when it is running,
    /// or the future's [`Output`], when the future has completed.
    ///
    /// [`Future`]: core::future::Future
    /// [`Output`]: core::future::Future::Output
    inner: UnsafeCell<Cell<F>>,

    /// The [`Waker`] of the [`JoinHandle`] for this task, if one exists.
    ///
    /// # Safety
    ///
    /// This field is only initialized when the [`State::JOIN_WAKER`] state
    /// field is set to `JoinWakerState::Waiting`. If the join waker state is
    /// any other value, this field may be uninitialized.
    ///
    /// [`State::JOIN_WAKER`]: state::State::JOIN_WAKER
    join_waker: UnsafeCell<CheckedMaybeUninit<Waker>>,

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
/// See the [`Vtable` documentation](Vtable#task-vtables) for  more details on a
/// task's vtables.
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
    /// See the [`Vtable` documentation](Vtable#task-vtables) for
    /// more details on a task's vtables.
    ///
    /// [waker vtable]: core::task::RawWakerVTable
    vtable: &'static Vtable,

    /// The task's `tracing` span, if `tracing` is enabled.
    span: trace::Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollResult {
    /// The task has completed, without waking a [`JoinHandle`] waker.
    ///
    /// The scheduler can increment a counter of completed tasks, and then drop
    /// the [`TaskRef`].
    Ready,

    /// The task has completed and a [`JoinHandle`] waker has been woken.
    ///
    /// The scheduler can increment a counter of completed tasks, and then drop
    /// the [`TaskRef`].
    ReadyJoined,

    /// The task is pending, but not woken.
    ///
    /// The scheduler can drop the [`TaskRef`], as whoever intends to wake the
    /// task later is holding a clone of its [`Waker`].
    Pending,

    /// The task has woken itself during the poll.
    ///
    /// The scheduler should re-schedule the task, rather than dropping the [`TaskRef`].
    PendingSchedule,
}

/// The task's [`Header`] and [scheduler] reference.
///
/// This is factored out into a separate type from `Task` itself so that we can
/// have a target for casting a pointer to that is generic only over the
/// `S`-typed [scheduler], and not the task's `Future` and `Storage` types. This
/// reduces excessive monomorphization of waker vtable functions.
///
/// This type knows the task's [`RawWaker`] vtable, as the raw waker methods
/// need only be generic over the type of the scheduler. It does not know the
/// task's *task* vtable, as the task vtable actually polls the future and
/// deallocates the task, and must therefore know the types of the task's future
/// and storage.
///
/// [scheduler]: crate::scheduler::Schedule
#[repr(C)]
struct Schedulable<S> {
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
}

/// The core of a task: either the [`Future`] that was spawned, if the task
/// has not yet completed, or the [`Output`] of the future, once the future has
/// completed.
///
/// [`Output`]: Future::Output
enum Cell<F: Future> {
    /// The future is still pending.
    Pending(F),
    /// The future has completed, and its output is ready to be taken by a
    /// `JoinHandle`, if one exists.
    Ready(F::Output),
    /// The future has completed, and the task's output has been taken or is not
    /// needed.
    Joined,
}

/// A [virtual function pointer table][vtable] (vtable) that specifies the
/// behavior of a [`Task`] instance.
///
/// This is distinct from the [`RawWakerVTable`] type in [`core::task`]: that
/// type specifies the vtable for a task's [`Waker`], while this vtable
/// specifies functions called by the runtime to poll, join, and deallocate a
/// spawned task.
///
/// The first argument passed to all functions inside this vtable is a pointer
/// to the task.
///
/// The functions inside this struct are only intended to be called on a pointer
/// to a spawned [`Task`]. Calling one of the contained functions using
/// any other pointer will cause undefined behavior.
///
/// ## Task Vtables
///
/// Each spawned task has two virtual function tables, which perform dynamic
/// dispatch on the type-erased type parameters of the task (the `S`-typed
/// [scheduler], the `F`-typed [`Future`], and the `STO`-typed [`Storage`]).
///
/// The first vtable is the [`RawWakerVTable`], which is specified by the Rust
/// standard library's [`core::task`] module. This vtable contains function
/// pointers to the implementations of the task's [`Waker`] operations. The
/// second vtable is the **task** vtable, which contains function pointers to
/// functions that are specific to the task's [`Future`] type, such as polling
/// the future and deallocating the task.
///
/// The [`RawWakerVTable`] is monomorphic only over the `S`-typed [`Schedule`]
/// implementation, so all tasks spawned on the same type of [scheduler] share
/// one instance of the [`RawWakerVTable`]. On the other hand, the task vtable
/// is monomorphic over the task's `F`-typed [`Future`] and `S`-typed
/// [`Storage`], so a separate monomorphization of the task vtable methods is
/// generated for each spawned [`Future`] type.
///
/// The task vtable is generated by the [`Task`] struct, as it requires type
/// information about the task's [`Future`] and [`Storage`], while the
/// [`RawWakerVTable`] is generated by the [`Schedulable`] struct, as it only
/// requires type information about the [`Schedule`] type. This reduces
/// unnecessary monomorphization of the waker vtable methods for each future
/// type that's spawned.
///
/// The methods contained in each vtable are as follows:
///
/// #### [`RawWakerVTable`]
///
/// * **`unsafe fn `[`clone`]`(*const ()) -> `[`RawWaker`]**
///
///   Called when a task's [`Waker`] is cloned.
///
///   Increments the task's reference count.
///
/// * **`unsafe fn `[`wake`]`(*const ())`**
///
///   Called when a task is woken by value.
///
///   Decrements the task's reference count.
///
/// * **`unsafe fn `[`wake_by_ref`]`(*const ())`**
///
///   Called when a task's [`Waker`] is woken through a reference.
///
///   This wakes the task but does not change the task's reference count.
///
/// * **`unsafe fn `[`drop`]`(*const ())`**
///
///   Called when a task's [`Waker`] is dropped.
///
///   Decrements the task's reference count.
///
/// #### Task `Vtable`
///
/// * **`unsafe fn `[`poll`]`(`[`NonNull`]`<`[`Header`]`>) -> `[`PollResult`]**
///
///   Polls the task's [`Future`].
///
///   This does *not* consume a [`TaskRef`], as the scheduler may wish to do
///   additional operations on the task even if it should be dropped. Instead,
///   this function returns a [`PollResult`] that indicates what the scheduler
///   should do with the task after the poll.
///
/// * **`unsafe fn `[`poll_join`]`(`[`NonNull`]`<`[`Header`]`>, `[`NonNull`]`<()>,
///   &mut `[`Context`]`<'_>) -> `[`Poll`]`<Result<(), `[`JoinError`]`>>`**
///
///   Called when a task's [`JoinHandle`] is polled.
///
///   This takes a `NonNull<Header>` rather than a [`TaskRef`], as it does not
///   consume a ref  count. The second [`NonNull`] is an out-pointer to which the
///   task's output will be written if the task has completed. The caller is
///   responsible for
///   ensuring that this points to a valid, if uninitialized, memory location
///   for a `F::Output`.
///
///   This method returns [`Poll::Ready`]`(Ok(()))` when the task has joined,
///   [`Poll::Ready`]`(Err(`[`JoinError`]`))` if the task has been cancelled, or
///   [`Poll::Pending`]` when the task is still running.
///
/// * **`unsafe fn `[`deallocate`]`(`[`NonNull`]`<`[`Header`]`>)`**
///
///   Called when a task's final [`TaskRef`] is dropped and the task is ready to
///   be deallocated.
///
///   This does not take a [`TaskRef`], as dropping a [`TaskRef`] decrements the
///   reference count, and the final `TaskRef` has already been dropped.
///
/// [scheduler]: crate::scheduler::Schedule
/// [task storage]: Storage
/// [dynamic dispatch]: https://en.wikipedia.org/wiki/Dynamic_dispatch
/// [vtable]: https://en.wikipedia.org/wiki/Virtual_method_table
/// [`clone`]: core::task::RawWakerVTable#clone
/// [`wake`]: core::task::RawWakerVTable#wake
/// [`wake_by_ref`]: core::task::RawWakerVTable#wake_by_ref
/// [`drop`]: core::task::RawWakerVTable#drop
/// [`poll`]: Task::poll
/// [`poll_join`]: Task::poll_join
/// [`deallocate`]: Task::deallocate
struct Vtable {
    /// Poll the future, returning a [`PollResult`] that indicates what the
    /// scheduler should do with the polled task.
    poll: unsafe fn(NonNull<Header>) -> PollResult,

    /// Poll the task's `JoinHandle` for completion, storing the output at the
    /// provided [`NonNull`] pointer if the task has completed.
    ///
    /// If the task has not completed, the [`Waker`] from the provided
    /// [`Context`] is registered to be woken when the task completes.
    // Splitting this up into type aliases just makes it *harder* to understand
    // IMO...
    #[allow(clippy::type_complexity)]
    poll_join:
        unsafe fn(NonNull<Header>, NonNull<()>, &mut Context<'_>) -> Poll<Result<(), JoinError>>,

    /// Drops the task and deallocates its memory.
    deallocate: unsafe fn(NonNull<Header>),
}

// === impl Task ===

macro_rules! trace_waker_op {
    ($ptr:expr, $method: ident) => {
        trace_waker_op!($ptr,  $method, op: $method)
    };
    ($ptr:expr, $method: ident, op: $op:ident) => {

        #[cfg(any(feature = "tracing-01", loom))]
        tracing_01::trace!(
            target: "runtime::waker",
            {
                task.id = (*$ptr).span().tracing_01_id(),
                task.addr = ?$ptr,
                op = concat!("waker.", stringify!($op)),
            },
            concat!("Task::", stringify!($method)),
        );


        #[cfg(not(any(feature = "tracing-01", loom)))]
        trace!(
            target: "runtime::waker",
            {
                task.addr = ?$ptr,
                op = concat!("waker.", stringify!($op)),
            },
            concat!("Task::", stringify!($method)),

        );
    };
}

impl<S, F, STO> Task<S, F, STO>
where
    F: Future,
{
    #[inline]
    fn header(&self) -> &Header {
        &self.schedulable.header
    }

    #[inline]
    fn state(&self) -> &StateCell {
        &self.header().state
    }

    #[inline]
    #[cfg(any(feature = "tracing-01", feature = "tracing-02", test))]
    fn span(&self) -> &trace::Span {
        &self.header().span
    }
}

impl<S, F, STO> Task<S, F, STO>
where
    S: Schedule,
    F: Future,
    STO: Storage<S, F>,
{
    const TASK_VTABLE: Vtable = Vtable {
        poll: Self::poll,
        poll_join: Self::poll_join,
        deallocate: Self::deallocate,
    };

    /// Create a new (non-heap-allocated) Task.
    ///
    /// This needs to be heap allocated using an implementor of
    /// the [`Storage`] trait to be used with the scheduler.
    ///
    /// [`Storage`]: crate::task::Storage
    pub fn new(scheduler: S, future: F) -> Self {
        Self {
            schedulable: Schedulable {
                header: Header {
                    run_queue: mpsc_queue::Links::new(),
                    vtable: &Self::TASK_VTABLE,
                    state: StateCell::new(),
                    span: crate::trace::Span::none(),
                },
                scheduler,
            },
            inner: UnsafeCell::new(Cell::Pending(future)),
            join_waker: UnsafeCell::new(CheckedMaybeUninit::uninit()),
            storage: PhantomData,
        }
    }

    unsafe fn poll(ptr: NonNull<Header>) -> PollResult {
        trace!(
            task.addr = ?ptr,
            task.output = %type_name::<<F>::Output>(),
            "Task::poll"
        );
        let mut this = ptr.cast::<Self>();
        test_debug!(task = ?fmt::alt(this.as_ref()));
        // try to transition the task to the polling state
        let state = &this.as_ref().state();
        match test_dbg!(state.start_poll()) {
            // transitioned successfully!
            Ok(_) => {}
            Err(_state) => {
                // TODO(eliza): could run the dealloc glue here instead of going
                // through a ref cycle?
                return PollResult::Ready;
            }
        }

        // wrap the waker in `ManuallyDrop` because we're converting it from an
        // existing task ref, rather than incrementing the task ref count. if
        // this waker is consumed during the poll, we don't want to decrement
        // its ref count when the poll ends.
        let waker = {
            let raw = Schedulable::<S>::raw_waker(this.as_ptr().cast());
            mem::ManuallyDrop::new(Waker::from_raw(raw))
        };

        // actually poll the task
        let poll = {
            let cx = Context::from_waker(&waker);
            let pin = Pin::new_unchecked(this.as_mut());
            pin.poll_inner(cx)
        };

        // post-poll state transition
        let result = test_dbg!(state.end_poll(poll.is_ready()));

        // if the task is ready and has a `JoinHandle` to wake, wake the join
        // waker now.
        if result == PollResult::ReadyJoined {
            this.as_ref().join_waker.with_mut(|join_waker| unsafe {
                let join_waker = (*join_waker).assume_init_read();
                test_debug!(?join_waker, "waking");
                join_waker.wake();
            });
        }

        result
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

    unsafe fn poll_join(
        task: NonNull<Header>,
        outptr: NonNull<()>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), JoinError>> {
        trace!(
            task.addr = ?task,
            task.output = %type_name::<<F>::Output>(),
            "Task::poll_join"
        );
        let task = task.cast::<Self>().as_ref();
        match test_dbg!(task.state().try_join()) {
            JoinAction::TakeOutput => {
                task.inner.with_mut(|cell| {
                    match mem::replace(&mut *cell, Cell::Joined) {
                        Cell::Ready(output) => outptr.cast::<mem::MaybeUninit<F::Output>>().as_mut().write(output),
                        state => unreachable!("attempted to take join output on a task that has not completed! task: {task:?}; state: {state:?}"),
                    }
                });

                return Poll::Ready(Ok(()));
            }
            JoinAction::Register => {
                task.join_waker.with_mut(|waker| unsafe {
                    // safety: we now have exclusive permission to write to the
                    // join waker.
                    (*waker).write(cx.waker().clone());
                })
            }
            JoinAction::Reregister => {
                task.join_waker.with_mut(|waker| unsafe {
                    // safety: we now have exclusive permission to write to the
                    // join waker.
                    let waker = (*waker).assume_init_mut();
                    let my_waker = cx.waker();
                    if !waker.will_wake(my_waker) {
                        *waker = my_waker.clone();
                    }
                });
            }
        }
        task.state().set_join_waker_registered();
        Poll::Pending
    }

    fn poll_inner(&self, mut cx: Context<'_>) -> Poll<()> {
        #[cfg(any(feature = "tracing-01", feature = "tracing-02", test))]
        let _span = self.span().enter();

        self.inner.with_mut(|cell| {
            let cell = unsafe { &mut *cell };
            let poll = match cell {
                Cell::Pending(future) => unsafe { Pin::new_unchecked(future).poll(&mut cx) },
                _ => unreachable!("tried to poll a completed future!"),
            };

            match poll {
                Poll::Ready(ready) => {
                    *cell = Cell::Ready(ready);
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
            .field("header", &self.header())
            .field("inner", &format_args!("UnsafeCell(<{}>)", type_name::<F>()))
            .field("join_waker", &format_args!("UnsafeCell(<Waker>)"))
            .field("scheduler", &fmt::display(type_name::<S>()))
            .field("storage", &fmt::display(type_name::<STO>()))
            .finish()
    }
}

impl<S, F, STO> Drop for Task<S, F, STO>
where
    F: Future,
{
    fn drop(&mut self) {
        // if there's a join waker, ensure that its destructor runs when the
        // task is dropped.
        // NOTE: this *should* never happen; we don't ever expect to deallocate
        // a task while it still has a `JoinHandle`, since the `JoinHandle`
        // holds a task ref. However, let's make sure we don't leak another task
        // in case something weird happens, I guess...
        if self.header().state.join_waker_needs_drop() {
            self.join_waker.with_mut(|waker| unsafe {
                // safety: we now have exclusive permission to write to the
                // join waker.
                (*waker).assume_init_drop();
            });
        }
    }
}

// === impl Schedulable ===

impl<S: Schedule> Schedulable<S> {
    /// The task's [`Waker`] vtable.
    ///
    /// This belongs to the `Schedulable` type rather than the [`Task`] type,
    /// because the [`Waker`] vtable methods need only be monomorphized over the
    /// `S`-typed [scheduler], and not over the task's `F`-typed [`Future`] or
    /// the `STO`-typed [`Storage`].
    ///
    /// [scheduler]: crate::scheduler::Schedule
    const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake_by_val,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    #[inline(always)]
    unsafe fn schedule(this: TaskRef) {
        this.0.cast::<Self>().as_ref().scheduler.schedule(this);
    }

    #[inline]
    unsafe fn drop_ref(this: NonNull<Self>) {
        trace!(
            task.addr = ?this,
            "Schedulable::drop_ref"
        );
        if !this.as_ref().state().drop_ref() {
            return;
        }

        let deallocate = this.as_ref().header.vtable.deallocate;
        deallocate(this.cast::<Header>())
    }

    fn raw_waker(this: *const Self) -> RawWaker {
        RawWaker::new(this as *const (), &Self::WAKER_VTABLE)
    }

    #[inline(always)]
    fn state(&self) -> &StateCell {
        &self.header.state
    }

    #[inline(always)]
    #[cfg(any(feature = "tracing-01", loom))]
    fn span(&self) -> &trace::Span {
        &self.header.span
    }

    // === Waker vtable methods ===

    unsafe fn wake_by_val(ptr: *const ()) {
        let ptr = ptr as *const Self;
        trace_waker_op!(ptr, wake_by_val, op: wake);

        let this = non_null(ptr as *mut Self);
        match test_dbg!(this.as_ref().state().wake_by_val()) {
            OrDrop::Drop => Self::drop_ref(this),
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
        trace_waker_op!(ptr, wake_by_ref);

        let this = non_null(ptr as *mut ()).cast::<Self>();
        if this.as_ref().state().wake_by_ref() == ScheduleAction::Enqueue {
            Self::schedule(TaskRef(this.cast::<Header>()));
        }
    }

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        let this = ptr as *const Self;
        trace_waker_op!(this, clone_waker, op: clone);
        (*this).header.state.clone_ref();
        Self::raw_waker(this)
    }

    unsafe fn drop_waker(ptr: *const ()) {
        let ptr = ptr as *const Self;
        trace_waker_op!(ptr, drop_waker, op: drop);

        let this = ptr as *mut _;
        Self::drop_ref(non_null(this))
    }
}

impl<S> fmt::Debug for Schedulable<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Schedulable")
            .field("header", &self.header)
            .field("scheduler", &fmt::display(type_name::<S>()))
            .finish()
    }
}

// === impl TaskRef ===

impl TaskRef {
    const NO_BUILDER: &'static Settings<'static> = &Settings::new();

    #[track_caller]
    pub(crate) fn new_allocated<S, F, STO>(task: STO::StoredTask) -> (Self, JoinHandle<F::Output>)
    where
        S: Schedule,
        F: Future,
        STO: Storage<S, F>,
    {
        Self::build_allocated::<S, F, STO>(Self::NO_BUILDER, task)
    }

    /// Returns a **non-owning** pointer to the referenced task's [`Header`].
    ///
    /// This does **not** modify the task's ref count, the [`TaskRef`] on which
    /// this function is called still owns a reference. Therefore, this means
    /// the returned [`NonNull`] pointer **may not** outlive this [`TaskRef`].
    ///
    /// # Safety
    ///
    /// The returned [`NonNull`] pointer is not guaranteed to be valid if it
    /// outlives the lifetime of this [`TaskRef`]. If this [`TaskRef`] is
    /// dropped, it *may* deallocate the task, and the [`NonNull`] pointer may
    /// dangle.
    ///
    /// **Do not** dereference the returned [`NonNull`] pointer unless at least
    /// one [`TaskRef`] referencing this task is known to exist!
    pub(crate) fn as_ptr(&self) -> NonNull<Header> {
        self.0
    }

    /// Convert a [`NonNull`] pointer to a task's [`Header`] into a new `TaskRef` to
    /// that task, incrementing the reference count.
    pub(crate) fn clone_from_raw(ptr: NonNull<Header>) -> Self {
        let this = Self(ptr);
        this.state().clone_ref();
        this
    }

    #[track_caller]
    pub(crate) fn build_allocated<S, F, STO>(
        builder: &Settings<'_>,
        task: STO::StoredTask,
    ) -> (Self, JoinHandle<F::Output>)
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
                ptr.as_mut().schedulable.header.span = span;
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
        let this = Self(ptr);
        let join_handle = unsafe {
            // Safety: it's fine to create a `JoinHandle` here, because we know
            // the task's actual output type.
            JoinHandle::from_task_ref(this.clone())
        };
        (this, join_handle)
    }

    pub(crate) fn poll(&self) -> PollResult {
        let poll_fn = self.header().vtable.poll;
        unsafe { poll_fn(self.0) }
    }

    /// # Safety
    ///
    /// `T` *must* be the task's actual output type!
    unsafe fn poll_join<T>(&self, cx: &mut Context<'_>) -> Poll<Result<T, JoinError>> {
        let poll_join_fn = self.header().vtable.poll_join;
        // NOTE: we can't use `CheckedMaybeUninit` here, since the vtable method
        // will cast this to a `MaybeUninit` and write to it; this would ignore
        // the initialized tracking bit.
        let mut slot = mem::MaybeUninit::<T>::uninit();
        match test_dbg!(poll_join_fn(
            self.0,
            NonNull::from(&mut slot).cast::<()>(),
            cx
        )) {
            Poll::Ready(Ok(())) => {
                // if the poll function returned `Ok`, we get to take the
                // output!
                Poll::Ready(Ok(slot.assume_init_read()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
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
    #[track_caller]
    fn clone(&self) -> Self {
        test_debug!("clone {:?}", self);
        self.state().clone_ref();
        Self(self.0)
    }
}

impl Drop for TaskRef {
    #[inline]
    #[track_caller]
    fn drop(&mut self) {
        test_debug!(task.addr = ?self.0, "drop TaskRef");
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
unsafe fn _maitake_header_nop(_ptr: NonNull<Header>) -> PollResult {
    #[cfg(debug_assertions)]
    unreachable!("stub task ({_ptr:?}) should never be polled!");
    #[cfg(not(debug_assertions))]
    PollResult::Pending
}

// See https://github.com/rust-lang/rust/issues/97708 for why
// this is necessary
#[no_mangle]
unsafe fn _maitake_header_nop_deallocate(ptr: NonNull<Header>) {
    unreachable!("stub task ({ptr:p}) should never be deallocated!");
}

// See https://github.com/rust-lang/rust/issues/97708 for why
// this is necessary
#[no_mangle]
unsafe fn _maitake_header_nop_poll_join(
    _ptr: NonNull<Header>,
    _: NonNull<()>,
    _: &mut Context<'_>,
) -> Poll<Result<(), JoinError>> {
    #[cfg(debug_assertions)]
    unreachable!("stub task ({_ptr:?}) should never be polled!");
    #[cfg(not(debug_assertions))]
    Poll::Ready(Err(JoinError::stub()))
}

impl Header {
    const STUB_VTABLE: Vtable = Vtable {
        poll: _maitake_header_nop,
        poll_join: _maitake_header_nop_poll_join,
        deallocate: _maitake_header_nop_deallocate,
    };

    #[cfg(not(loom))]
    pub(crate) const fn new_stub() -> Self {
        Self {
            run_queue: mpsc_queue::Links::new_stub(),
            state: StateCell::new(),
            vtable: &Self::STUB_VTABLE,
            span: trace::Span::none(),
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

// === impl Cell ===

impl<F: Future> fmt::Debug for Cell<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cell::Pending(_) => write!(f, "Cell::Pending({})", type_name::<F>()),
            Cell::Ready(_) => write!(f, "Cell::Ready({})", type_name::<F::Output>()),
            Cell::Joined => f.pad("Cell::Joined"),
        }
    }
}

// === impl Vtable ===

impl fmt::Debug for Vtable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vtable")
            .field("poll", &fmt::ptr(self.poll))
            .field("poll_join", &fmt::ptr(self.poll_join as *const ()))
            .field("deallocate", &fmt::ptr(self.deallocate))
            .finish()
    }
}

// Additional types and capabilities only available with the "alloc"
// feature active
feature! {
    #![feature = "alloc"]

    use alloc::boxed::Box;

    impl TaskRef {

        #[track_caller]
        pub(crate) fn new<S, F>(scheduler: S, future: F) -> (Self, JoinHandle<F::Output>)
        where
            S: Schedule,
            F: Future + 'static
        {
            let task = Box::new(Task::<S, F, BoxStorage>::new(scheduler, future));
            Self::build_allocated::<S, F, BoxStorage>(Self::NO_BUILDER, task)
        }
    }

}
