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
    links: UnsafeCell<list::Links<Entry>>,
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
                *this.registered = true;
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
