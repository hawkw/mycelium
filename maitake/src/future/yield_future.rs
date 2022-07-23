use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// A future that yields to the scheduler one or more times before completing.
#[derive(Debug)]
#[must_use = "futures do nothing unless `.await`ed or polled"]
pub struct Yield {
    yields: usize,
}

impl Future for Yield {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let yields = &mut self.as_mut().yields;
        if *yields == 0 {
            return Poll::Ready(());
        }
        *yields -= 1;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Yield {
    /// Returns a new future that yields `yields` times before completing.
    #[inline]
    pub const fn new(yields: usize) -> Self {
        Self { yields }
    }
}

/// Yield to the scheduler a single time before proceeding.
#[inline]
pub fn yield_now() -> Yield {
    Yield::new(1)
}
