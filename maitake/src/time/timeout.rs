//! [`Timeout`]s limit the amount of time a [`Future`] is allowed to run before
//! it completes.
//!
//! See the documentation for the [`Timeout`] type for details.
use super::{Sleep, Timer};
use core::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use pin_project::pin_project;

/// A [`Future`] that requires an inner [`Future`] to complete within a
/// specified [`Duration`].
///
/// This `Future` is returned by the [`timeout`](super::timeout) function, and
/// by [`Timeout::new`].
///
/// # Output
///
/// - [`Ok`]`(F::Output)` if the inner future completed before the specified
///   timeout.
/// - [`Err`]`(`[`Elapsed`]`)` if the timeout elapsed before the inner [`Future`]
///   completed.
///
/// # Cancellation
///
/// Dropping a `Timeout` future cancels the timeout. The wrapped [`Future`] can
/// be extracted from the `Timeout` future by calling [`Timeout::into_inner`],
/// allowing the future to be polled without failing if the timeout elapses.
#[derive(Debug)]
#[pin_project]
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
pub struct Timeout<'timer, F> {
    #[pin]
    sleep: Sleep<'timer>,
    #[pin]
    future: F,
    duration: Duration,
}

/// An error indicating that a [`Timeout`] elapsed before the inner [`Future`]
/// completed.
#[derive(Debug)]
pub struct Elapsed(Duration);

// === impl Timeout ===

impl<'timer, F: Future> Timeout<'timer, F> {
    /// Returns a new [`Timeout`] future that fails if `future` does not
    /// complete within the specified `duration`.
    ///
    /// The timeout will be driven by the specified `timer`.
    ///
    /// See the documentation for the [`Timeout`] future for details.
    pub fn new(timer: &'timer Timer, future: F, duration: Duration) -> Self {
        Self {
            sleep: timer.sleep(duration),
            future,
            duration,
        }
    }

    /// Consumes this `Timeout`, returning the inner [`Future`].
    ///
    /// This can be used to continue polling the inner [`Future`] without
    /// requiring it to complete prior to the specified timeout.
    pub fn into_inner(self) -> F {
        self.future
    }

    /// Borrows the inner [`Future`] immutably.
    pub fn get_ref(&self) -> &F {
        &self.future
    }

    /// Mutably the inner [`Future`].
    pub fn get_mut(&mut self) -> &mut F {
        &mut self.future
    }

    /// Borrows the inner [`Future`] as a [`Pin`]ned reference, if this
    /// `Timeout` is pinned.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut F> {
        self.project().future
    }

    /// Returns the [`Duration`] the inner [`Future`] is allowed to run for.
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

impl<F: Future> Future for Timeout<'_, F> {
    type Output = Result<F::Output, Elapsed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        // first, poll the sleep.
        if this.sleep.poll(cx).is_ready() {
            return Poll::Ready(Err(Elapsed(*this.duration)));
        }

        // then, try polling the future.
        if let Poll::Ready(output) = this.future.poll(cx) {
            return Poll::Ready(Ok(output));
        }

        Poll::Pending
    }
}

// === impl Elapsed ===

impl From<Elapsed> for Duration {
    #[inline]
    fn from(Elapsed(duration): Elapsed) -> Self {
        duration
    }
}

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timed out after {:?}", self.0)
    }
}