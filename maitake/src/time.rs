//! Time utilities.
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
/// # Panics
///
/// - If a global timer was not set by calling [`timer::set_global_default`] first.
/// - If the provided duration exceeds the maximum sleep duration allowed by the
///   global default timer.
#[track_caller]

pub fn timeout<F: Future>(duration: Duration, future: F) -> Timeout<'static, F> {
    Timeout::new(timer::global::default(), future, duration)
}
