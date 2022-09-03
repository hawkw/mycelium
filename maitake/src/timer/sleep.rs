use super::*;
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
};
use pin_project::{pin_project, pinned_drop};

#[pin_project(PinnedDrop)]
pub struct Sleep<'timer> {
    state: State,
    timer: &'timer Mutex<Core>,
    #[pin]
    entry: Entry,
}

#[derive(Debug)]
#[repr(C)]
#[pin_project]
pub(super) struct Entry {
    /// Intrusive linked list pointers.
    #[pin]
    links: UnsafeCell<list::Links<Entry>>,

    pub(super) waker: WaitCell,

    pub(super) ticks: Ticks,

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
    pub(super) fn new(core: &'timer Mutex<Core>, ticks: Ticks) -> Self {
        Self {
            state: State::Unregistered,
            timer: core,
            entry: Entry {
                links: UnsafeCell::new(list::Links::new()),
                waker: WaitCell::new(),
                ticks,
                _pin: PhantomPinned,
            },
        }
    }
}

impl Future for Sleep<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.as_mut().project();
        // If necessary, register the sleep
        match test_dbg!(*this.state) {
            State::Unregistered => {
                let ptr =
                    unsafe { ptr::NonNull::from(Pin::into_inner_unchecked(this.entry.as_mut())) };
                this.timer.lock().insert_sleep(ptr);
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
        if test_dbg!(*this.state) == State::Registered {
            if this.entry.as_ref().project_ref().waker.is_closed() {
                return;
            }
            this.timer.lock().cancel_sleep(this.entry);
        }
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
