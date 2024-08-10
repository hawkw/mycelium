/// An [exponential backoff] for spin loops.
///
/// This is a helper struct for spinning in a busy loop, with an exponentially
/// increasing number of spins up to a maximum value.
///
/// [exponential backoff]: https://en.wikipedia.org/wiki/Exponential_backoff
#[derive(Debug, Copy, Clone)]
pub struct Backoff {
    exp: u8,
    max: u8,
}

// === impl Backoff ===

impl Backoff {
    /// The default maximum exponent (2^8).
    ///
    /// This is the maximum exponent returned by [`Backoff::new()`] and
    /// [`Backoff::default()`]. To override the maximum exponent, use
    /// [`Backoff::with_max_exponent()`].
    pub const DEFAULT_MAX_EXPONENT: u8 = 8;

    /// Returns a new exponential backoff with the maximum exponent set to
    /// [`Self::DEFAULT_MAX_EXPONENT`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            exp: 0,
            max: Self::DEFAULT_MAX_EXPONENT,
        }
    }

    /// Returns a new exponential backoff with the provided max exponent.
    #[must_use]
    pub fn with_max_exponent(max: u8) -> Self {
        assert!(max <= Self::DEFAULT_MAX_EXPONENT);
        Self { exp: 0, max }
    }

    /// Backs off in a spin loop.
    ///
    /// This should be used when an operation needs to be retried because
    /// another thread or core made progress. Depending on the target
    /// architecture, this will generally issue a sequence of `yield` or `pause`
    /// instructions.
    ///
    /// Each time this function is called, it will issue `2^exp` [spin loop
    /// hints], where `exp` is the current exponent value (starting at 0). If
    /// `exp` is less than the configured maximum exponent, the exponent is
    /// incremented once the spin is complete.
    #[inline(always)]
    pub fn spin(&mut self) {
        // Issue 2^exp pause instructions.
        #[cfg_attr(loom, allow(unused_variables))]
        let spins = 1 << self.exp;

        #[cfg(not(loom))]
        for _ in 0..spins {
            crate::loom::hint::spin_loop();
        }

        #[cfg(loom)]
        {
            test_debug!("would back off for {spins} spins");
            loom::thread::yield_now();
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
