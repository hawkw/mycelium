use super::{Ticks, Timer};
use crate::{
    loom::cell::UnsafeCell,
    sync::wait_cell::{self, WaitCell},
};
use cordyceps::{list, Linked};
use core::{
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    ptr::{self, NonNull},
    task::{Context, Poll},
    time::Duration,
};
use mycelium_util::fmt;
use pin_project::{pin_project, pinned_drop};

/// A [`Future`] that completes after a specified [`Duration`].
///
/// This `Future` is returned by the [`sleep`] and [`try_sleep`] functions,
/// and by the [`Timer::sleep`] and [`Timer::try_sleep`] methods.
///
/// [`timeout`]: crate::time::sleep
/// [`try_sleep`]: super::try_sleep
#[pin_project(PinnedDrop)]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Sleep<'timer> {
    state: State,
    timer: &'timer Timer,
    #[pin]
    entry: Entry,
}

#[derive(Debug)]
#[pin_project]
pub(super) struct Entry {
    /// Intrusive linked list pointers.
    #[pin]
    links: UnsafeCell<list::Links<Entry>>,

    pub(super) waker: WaitCell,

    pub(super) ticks: Ticks,

    /// The wheel's elapsed timestamp when this `sleep` future was first polled.
    ///
    /// # Safety
    ///
    /// This is safe to access under the following conditions:
    ///
    /// * It may be written to only while holding the wheel lock, if the future
    ///   is in the [`State::Unregistered`] state.
    /// * It may be read at any point once the future is in the
    ///   [`State::Registered`] state. It may never be written again after the
    ///   state has progressed to `Registered`.
    ///
    /// It would be nice if this could just be an `AtomicU64` but LOLSOB WE CANT
    /// HAVE NICE THINGS BECAUSE OF CORTEX-M.
    pub(super) deadline: UnsafeCell<Ticks>,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum State {
    Unregistered,
    Registered,
    Completed,
}

// === impl Sleep ===

impl<'timer> Sleep<'timer> {
    pub(super) fn new(timer: &'timer Timer, ticks: Ticks) -> Self {
        Self {
            state: State::Unregistered,
            timer,
            entry: Entry {
                links: UnsafeCell::new(list::Links::new()),
                waker: WaitCell::new(),
                deadline: UnsafeCell::new(0),
                ticks,
                _pin: PhantomPinned,
            },
        }
    }

    /// Returns the [`Duration`] that this `Sleep` future will sleep for.
    pub fn duration(&self) -> Duration {
        self.timer.ticks_to_dur(self.entry.ticks)
    }
}

impl Future for Sleep<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.as_mut().project();
        trace!(self.addr = ?format_args!("{:p}", this.entry), "Sleep::poll");
        // If necessary, register the sleep
        match *this.state {
            State::Unregistered => {
                let ptr =
                    unsafe { ptr::NonNull::from(Pin::into_inner_unchecked(this.entry.as_mut())) };
                this.timer.core().register_sleep(ptr);
                *this.state = State::Registered;
            }
            State::Registered => {}
            State::Completed => return Poll::Ready(()),
        }

        match this.entry.waker.register_wait(cx.waker()) {
            Ok(_) => Poll::Pending,
            // the timer has fired, so the future has now completed.
            Err(wait_cell::RegisterError::Closed) => {
                *this.state = State::Completed;
                Poll::Ready(())
            }
            // these ones don't happen
            Err(wait_cell::RegisterError::Registering) => {
                unreachable!("a sleep should only be polled by one task!")
            }
            Err(wait_cell::RegisterError::Waking) => {
                unreachable!("a sleep's WaitCell should only be woken by closing")
            }
        }
    }
}

#[pinned_drop]
impl PinnedDrop for Sleep<'_> {
    fn drop(mut self: Pin<&mut Self>) {
        let this = self.project();
        // we only need to remove the sleep from the timer wheel if it's
        // currently part of a linked list --- if the future hasn't been polled
        // yet, or it has already completed, we don't need to lock the timer to
        // remove it.
        if *this.state == State::Registered {
            if this.entry.as_ref().project_ref().waker.is_closed() {
                return;
            }
            this.timer.core().cancel_sleep(this.entry);
        }
    }
}

impl fmt::Debug for Sleep<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sleep")
            .field("duration", &self.duration())
            .field("state", &self.state)
            .field("addr", &fmt::ptr(&self.entry))
            .field("timer", &fmt::ptr(&self.timer))
            .finish()
    }
}

// === impl Entry ===

unsafe impl Linked<list::Links<Entry>> for Entry {
    type Handle = NonNull<Entry>;

    fn into_ptr(r: Self::Handle) -> NonNull<Self> {
        r
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

    unsafe fn links(target: NonNull<Self>) -> NonNull<list::Links<Entry>> {
        // Safety: using `ptr::addr_of!` avoids creating a temporary
        // reference, which stacked borrows dislikes.
        let links = ptr::addr_of!((*target.as_ptr()).links);
        (*links).with_mut(|links| {
            // Safety: since the `target` pointer is `NonNull`, we can assume
            // that pointers to its members are also not null, making this use
            // of `new_unchecked` fine.
            NonNull::new_unchecked(links)
        })
    }
}

impl Entry {
    pub(super) fn fire(&self) {
        trace!(timer.addr = ?fmt::ptr(self), "firing timer");
        self.waker.close();
    }
}
