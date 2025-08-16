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
//! The [`Sleep`] and [`Timeout`] futures do not complete on their own. Instead,
//! they must be driven by a [`Timer`], which tracks the current time and
//! notifies time-based futures when their deadlines are reached.
//!
//! The [`Timer`] struct implements a [hierarchical timer wheel][wheel], a data
//! structure for tracking large numbers of timers efficiently. It is used to
//! create [`Sleep`]s and [`Timeout`]s, and notify them when their deadlines
//! complete. In order to be used, a [`Timer`] must be driven by a hardware time
//! source. See [the `Timer` documentation][driving-timers] for more information
//! on using this type to implement a system timer.
//!
//! ### Global Timers
//!
//! In most cases, it is desirable to have a single global timer instance that
//! drives all time-based futures in the system. In particular, creating new
//! [`Sleep`] and [`Timeout`] futures typically requires a reference to a
//! [`Timer`] instance, which can be inconvenient to pass around to the points
//! in a system where [`Sleep`] and [`Timeout`] futures are created.
//!
//! Therefore, the `maitake` timer also includes support for setting a global
//! timer, using the [`set_global_timer`] function. Once a global timer
//! has been initialized, the [`sleep()`] and [`timeout()`] free functions in
//! this module can be used to create time-based futures without a reference to a
//! [`Timer`] instance. These functions will always create futures bound to the
//! global default timer.
//!
//! Note that a global default timer can only be set once. Subsequent calls to
//! [`set_global_timer`] after a global timer has been initialized will
//! return an error.
//!
//! [wheel]: http://www.cs.columbia.edu/~nahum/w6998/papers/sosp87-timing-wheels.pdf
//! [driving-timers]: Timer#driving-timers
#![warn(missing_docs, missing_debug_implementations)]
pub mod clock;
pub mod timeout;
mod timer;

use crate::util;

#[doc(inline)]
pub use self::{
    clock::{Clock, Instant},
    timeout::Timeout,
    timer::{set_global_timer, sleep::Sleep, AlreadyInitialized, Timer, TimerError, Turn},
};
pub use core::time::Duration;

use core::future::Future;

/// Returns a [`Future`] that completes after the specified [`Duration`].
///
/// This function uses the [global default timer][global], and the returned
/// [`Sleep`] future will live for the `'static` lifetime. See [the
/// module-level documentation][global] for details on using the global default
/// timer.
///
/// # Panics
///
/// - If a [global timer][global] was not set by calling [`set_global_timer`]
///   first.
/// - If the provided duration exceeds the [maximum sleep duration][max] allowed
///   by the global default timer.
///
/// For a version of this function that does not panic, see [`try_sleep()`]
/// instead.
///
/// # Examples
///
/// ```
/// use maitake::time;
///
/// async fn example() {
///     time::sleep(time::Duration::from_secs(1)).await;
///     println!("one second has passed!");
/// }
/// ```
///
/// [global]: #global-timers
/// [max]: Timer::max_duration
#[track_caller]
pub fn sleep(duration: Duration) -> Sleep<'static> {
    util::expect_display(try_sleep(duration), "cannot create `Sleep` future")
}

/// Returns a [`Future`] that completes after the specified [`Duration`].
///
/// This function uses the [global default timer][global], and the returned
/// [`Timeout`] future will live for the `'static` lifetime. See [the
/// module-level documentation][global] for details on using the global default
/// timer.
///
/// # Returns
///
/// - [`Ok`]`(`[`Sleep`]`)` if a new [`Sleep`] future was created
///   successfully.
/// - [`Err`]`(`[`TimerError::NoGlobalTimer`]`)` if a [global timer][global] was
///   not set by calling [`set_global_timer`] first.
/// - [`Err`]`(`[`TimerError::DurationTooLong`]`)` if the requested sleep
///   duration exceeds the [global timer][global]'s [maximum sleep
///   duration](Timer::max_duration`).
///
/// # Panics
///
/// This function does not panic. For a version of this function that panics
/// rather than returning a [`TimerError`], use [`sleep()`] instead.
///
/// # Examples
///
/// ```
/// use maitake::time;
///
/// async fn example() {
///     // try to sleep for one second
///     match time::try_sleep(time::Duration::from_secs(1)) {
///         // the sleep future was created successfully, so we can now await it.
///         Ok(sleep) => {
///             sleep.await;
///             println!("one second has passed!");
///         },
///         Err(time::TimerError::NoGlobalTimer) =>
///             println!("timer is not initialized"),
///         Err(time::TimerError::DurationTooLong { .. }) =>
///             unreachable!("1 second should not exceed the max duration"),
///         Err(error) => panic!("unexpected timer error: {error}"),
///     }
/// }
/// ```
///
/// [global]: #global-timers
pub fn try_sleep(duration: Duration) -> Result<Sleep<'static>, TimerError> {
    timer::global::default()?.try_sleep(duration)
}

/// Requires the provided [`Future`] to complete before the specified [`Duration`]
/// has elapsed.
///
/// This function uses the [global default timer][global], and the returned
/// [`Timeout`] future will live for the `'static` lifetime. See [the
/// module-level documentation][global] for details on using the global default
/// timer.
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
/// - If a [global timer][global] was not set by calling [`set_global_timer`]
///   first.
/// - If the provided duration exceeds the [maximum sleep duration][max] allowed
///   by the global default timer.
///
/// For a version of this function that does not panic, use the [`try_timeout()`]
/// function instead.
///
/// # Examples
///
/// ```
/// use maitake::time::{timeout, Duration};
///
/// /// A function that might wait for a long time before it completes.
/// async fn do_slow_stuff() {
///    // do some slow stuff ...
/// }
///
/// async fn example() {
///     // try to do some slow stuff, but if it takes longer than 10 seconds,
///     // give up.
///     match timeout(Duration::from_secs(10), do_slow_stuff()).await {
///         Ok(_) => println!("slow stuff completed successfully"),
///         Err(elapsed) =>
///             eprintln!("slow stuff did not complete in {:?}!", elapsed.duration()),
///     }
/// }
/// ```
///
/// [global]: #global-timers
/// [max]: Timer::max_duration
/// [`Elapsed`]: timeout::Elapsed
#[track_caller]
pub fn timeout<F: Future>(duration: Duration, future: F) -> Timeout<'static, F> {
    util::expect_display(
        try_timeout(duration, future),
        "cannot create `Timeout` future",
    )
}

/// Requires the provided [`Future`] to complete before the specified [`Duration`]
/// has elapsed.
///
/// This function uses the [global default timer][global], and the returned
/// [`Timeout`] future will live for the `'static` lifetime. See [the
/// module-level documentation][global] for details on using the global default
/// timer.
///
/// # Returns
///
/// - [`Ok`]`(`[`Timeout`]`)` if a new [`Timeout`] future was created
///   successfully.
/// - [`Err`]`(`[`TimerError::NoGlobalTimer`]`)` if a [global timer][global] was
///   not set by calling [`set_global_timer`] first.
/// - [`Err`]`(`[`TimerError::DurationTooLong`]`)` if the requested timeout
///   duration exceeds the [global timer][global]'s [maximum sleep
///   duration](Timer::max_duration`).
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
/// This function does not panic. For a version of this function that panics
/// rather than returning a [`TimerError`], use [`timeout()`] instead.
///
/// # Examples
///
/// ```
/// use maitake::time::{self, Duration};
///
/// /// A function that might wait for a long time before it completes.
/// async fn do_slow_stuff() {
///    // do some slow stuff ...
/// }
///
/// async fn example() {
///     // if we can't create the timeout, just wait for the future to complete
///     // with no timeout.
///     match time::try_timeout(Duration::from_secs(10), do_slow_stuff()) {
///         // we successfully created a timeout, so await the timeout future.
///         Ok(timeout) => match timeout.await {
///             Ok(_) => {},
///             Err(elapsed) => {
///                 eprintln!("slow stuff did not complete in {:?}", elapsed.duration());
///                 return;
///             },
///         },
///         // a timeout could not be created, so just try the slow stuff
///         // without setting the timeout.
///         Err(_) => do_slow_stuff().await,
///     };
///
///     println!("slow stuff completed successfully");
/// }
/// ```
///
/// [global]: #global-timers
/// [`Elapsed`]: timeout::Elapsed
pub fn try_timeout<F: Future>(
    duration: Duration,
    future: F,
) -> Result<Timeout<'static, F>, timer::TimerError> {
    timer::global::default()?.try_timeout(duration, future)
}
