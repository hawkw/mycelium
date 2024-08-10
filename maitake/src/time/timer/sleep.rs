use super::{Ticks, Timer};
use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering::*},
    },
    sync::wait_cell::WaitCell,
};
use cordyceps::{list, Linked};
use core::{
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    ptr::{self, NonNull},
    task::{ready, Context, Poll},
    time::Duration,
};
use mycelium_util::fmt;
use pin_project::{pin_project, pinned_drop};

/// A [`Future`] that completes after a specified [`Duration`].
///
/// This `Future` is returned by the [`sleep`] and [`try_sleep`] functions,
/// and by the [`Timer::sleep`] and [`Timer::try_sleep`] methods.
///
/// [`sleep`]: crate::time::sleep
/// [`try_sleep`]: crate::time::try_sleep
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

    /// The waker of the task awaiting this future.
    waker: WaitCell,

    /// The wheel's elapsed timestamp when this `sleep` future was first polled.
    pub(super) deadline: Ticks,

    pub(in crate::time::timer) ticks: Ticks,

    pub(super) linked: AtomicBool,

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
        let now = timer.clock().now_ticks();
        let deadline = now + ticks;
        debug!(
            target: "maitake::time::sleep",
            now,
            sleep.ticks = ticks,
            sleep.deadline = deadline,
            "Sleep::new({ticks})"
        );
        Self {
            state: State::Unregistered,
            timer,
            entry: Entry {
                links: UnsafeCell::new(list::Links::new()),
                waker: WaitCell::new(),
                deadline,
                ticks,
                linked: AtomicBool::new(false),
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
        trace!(
            target: "maitake::time::sleep",
            addr = ?fmt::ptr(&*this.entry),
            state = ?*this.state,
            "Sleep::poll"
        );
        // If necessary, register the sleep
        match test_dbg!(*this.state) {
            State::Unregistered => {
                let ptr =
                    unsafe { ptr::NonNull::from(Pin::into_inner_unchecked(this.entry.as_mut())) };
                // Acquire the wheel lock to insert the sleep.
                let done = this.timer.core.with_lock(|core| {
                    // While we are holding the wheel lock, go ahead and advance the
                    // timer, too. This way, the timer wheel gets advanced more
                    // frequently than just when a scheduler tick completes or a
                    // timer IRQ fires, helping to increase timer accuracy.
                    this.timer.advance_locked(core);

                    match test_dbg!(core.register_sleep(ptr)) {
                        Poll::Ready(()) => {
                            *this.state = State::Completed;
                            true
                        }
                        Poll::Pending => {
                            *this.state = State::Registered;
                            false
                        }
                    }
                });
                if done {
                    return Poll::Ready(());
                }
            }
            State::Registered => {}
            State::Completed => return Poll::Ready(()),
        }

        let _poll = ready!(test_dbg!(this.entry.waker.poll_wait(cx)));
        debug_assert!(
            _poll.is_err(),
            "a Sleep's WaitCell should only be woken by closing"
        );
        Poll::Ready(())
    }
}

#[pinned_drop]
impl PinnedDrop for Sleep<'_> {
    fn drop(mut self: Pin<&mut Self>) {
        let this = self.project();
        trace!(sleep.addr = ?format_args!("{:p}", this.entry), "Sleep::drop");
        // we only need to remove the sleep from the timer wheel if it's
        // currently part of a linked list --- if the future hasn't been polled
        // yet, or it has already completed, we don't need to lock the timer to
        // remove it.
        if test_dbg!(this.entry.linked.load(Acquire)) {
            this.timer
                .core
                .with_lock(|core| core.cancel_sleep(this.entry));
        }
    }
}

impl fmt::Debug for Sleep<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            state,
            entry,
            timer,
        } = self;
        f.debug_struct("Sleep")
            .field("duration", &self.duration())
            .field("state", state)
            .field("addr", &fmt::ptr(entry))
            .field("timer", &fmt::ptr(*timer))
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
        trace!(sleep.addr = ?fmt::ptr(self), "firing sleep");
        self.waker.close();
        let _was_linked = self.linked.compare_exchange(true, false, AcqRel, Acquire);
        test_trace!(sleep.was_linked = _was_linked.is_ok());
    }
}
