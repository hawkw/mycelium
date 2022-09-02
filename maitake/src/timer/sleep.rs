use super::*;
use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering},
    },
    sync::wait_cell::{self, WaitCell},
};
use cordyceps::{list, Linked};
use core::{
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use pin_project::{pin_project, pinned_drop};

#[pin_project(PinnedDrop)]
pub struct Sleep<'timer> {
    registered: bool,
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
    node: UnsafeCell<list::Links<Entry>>,
    waker: WaitCell,
    ticks: Ticks,

    // This type is !Unpin due to the heuristic from:
    // <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

// === impl Sleep ===

impl Future for Sleep<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        if *this.registered {
            return Poll::Ready(());
        }

        match this.entry.waker.register_wait(cx.waker()) {
            Ok(_) => {
                this.registered = true;
                Poll::Pending
            }
            // timer firing while we were trying to register!
            Err(wait_cell::RegisterError::Waking) => Poll::Ready(()),
            // these ones don't happen
            Err(wait_cell::RegisterError::Registering) => {
                unreachable!("a sleep should only be polled by one task!")
            }
            Err(wait_cell::RegisterError::Closed) => {
                unreachable!("a sleep's WaitCell does not close!")
            }
        }
    }
}

#[pinned_drop]
impl PinnedDrop for Sleep<'_> {
    fn drop(mut self: Pin<&mut Self>) {
        let this = self.project();
        this.timer.lock().cancel_sleep(this.entry);
    }
}
