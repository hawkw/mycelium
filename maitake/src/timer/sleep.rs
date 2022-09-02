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
    marker::PhantomPin,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use pin_project::pin_project;

#[pin_project(PinnedDrop)]
pub struct Sleep {
    registered: bool,
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

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.registered {
            // We made it to "once", and got polled again, we must be ready!
            return Poll::Ready(Ok(()));
        }

        match self.waker.register_wait(cx.waker()) {
            Ok(_) => {
                self.registered = true;
                Poll::Pending
            }
            // timer firing while we were trying to register!
            Err(wait_cell::RegisterError::Waking) => Poll::Ready(Ok(())),
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
impl PinnedDrop for Sleep {
    fn drop(mut self: Pin<&mut Self>) {
        let this = self.project();
        this.waiter.release(this.queue);
    }
}
