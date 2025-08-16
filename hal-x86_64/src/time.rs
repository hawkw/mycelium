//! x86 hardware timers and timekeeping functionality.
pub(crate) mod pit;
mod tsc;
pub use self::{
    pit::{Pit, PitError, PIT},
    tsc::Rdtsc,
};
pub use core::time::Duration;

/// Error indicating that a [`Duration`] was invalid for a particular use.
#[derive(Copy, Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid duration {duration:?}: {message}")]
pub struct InvalidDuration {
    duration: Duration,
    message: &'static str,
}

// === impl InvalidDuration ===

impl InvalidDuration {
    /// Returns the [`Duration`] that was invalid.
    #[must_use]
    pub fn duration(self) -> Duration {
        self.duration
    }

    #[must_use]
    pub(crate) fn new(duration: Duration, message: &'static str) -> Self {
        Self { duration, message }
    }
}
