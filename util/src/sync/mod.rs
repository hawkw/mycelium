//! Synchronization primitives.

#[cfg(any(test, feature = "loom"))]
pub use loom::sync::atomic;

#[cfg(all(not(test), not(feature = "loom")))]
pub use core::sync::atomic;

pub mod once;
pub mod spin;
pub use self::once::{InitOnce, Lazy};

/// An exponential backoff for spin loops
#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    exp: u8,
    max: u8,
}

impl Backoff {
    pub(crate) const DEFAULT_MAX_EXPONENT: u8 = 8;

    pub(crate) const fn new() -> Self {
        Self {
            exp: 0,
            max: Self::DEFAULT_MAX_EXPONENT,
        }
    }

    /// Returns a new exponential backoff with the provided max exponent.
    #[allow(dead_code)]
    pub(crate) fn with_max_exponent(max: u8) -> Self {
        assert!(max <= Self::DEFAULT_MAX_EXPONENT);
        Self { exp: 0, max }
    }

    /// Perform one spin, squarin the backoff
    #[inline(always)]
    pub(crate) fn spin(&mut self) {
        // Issue 2^exp pause instructions.
        for _ in 0..(1 << self.exp) {
            atomic::spin_loop_hint();
        }

        if self.exp < self.max {
            self.exp += 1
        }
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}
