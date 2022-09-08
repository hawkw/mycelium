//! Utilities for tracking time and constructing system timers.
//!
//! # Futures
//!
//! This module contains the following [`Future`]s:
//!
//! - [`Sleep`], a future which completes after a specified duration,
//! - [`Timeout`], which wraps another [`Future`] to limit the duration it can
//!   run for.
//!
//! # Timers
//!
//! In order to use the [`Sleep`] and [`Timeout`] futures, they must be driven
//! by a system [`Timer`], which tracks the current time and notifies
//! time-based futures when their deadlines are reached.
//!
//! TODO(eliza): finish this part
//!
#![warn(missing_docs, missing_debug_implementations)]
pub mod timeout;
pub mod timer;

#[doc(inline)]
pub use self::timeout::Timeout;
pub use self::timer::{sleep::Sleep, Timer};
pub use core::time::Duration;

use core::future::Future;

/// Returns a [`Future`] that completes after the specified [`Duration`].
///
/// # Panics
///
/// - If a global timer was not set by calling [`timer::set_global_default`] first.
/// - If the provided duration exceeds the maximum sleep duration allowed by the
///   global default timer.

#[track_caller]

pub fn sleep(duration: Duration) -> Sleep<'static> {
    timer::global::default().sleep(duration)
}

/// Requires the provided [`Future`] to complete before the specified [`Duration`]
/// has elapsed.
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
///
/// # Panics
///
/// - If a global timer was not set by calling [`timer::set_global_default`] first.
/// - If the provided duration exceeds the maximum sleep duration allowed by the
///   global default timer.
#[track_caller]

pub fn timeout<F: Future>(duration: Duration, future: F) -> Timeout<'static, F> {
    Timeout::new(timer::global::default(), future, duration)
}
