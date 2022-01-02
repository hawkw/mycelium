//! Synchronization primitives.

#[cfg(loom)]
pub use loom::sync::atomic;

#[cfg(not(loom))]
pub use core::sync::atomic;

pub mod once;
pub mod spin;
pub use self::once::{InitOnce, Lazy};

pub mod hint {
    #[cfg(not(loom))]
    pub use core::hint::spin_loop;

    #[cfg(loom)]
    pub use loom::sync::atomic::spin_loop_hint as spin_loop;
}

/// An exponential backoff for spin loops
#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    exp: u8,
    max: u8,
}

#[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), repr(align(128)))]
#[cfg_attr(
    not(any(target_arch = "x86_64", target_arch = "aarch64")),
    repr(align(64))
)]
#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
pub struct CachePadded<T>(T);

// === impl Backoff ===

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
            hint::spin_loop();
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

// === impl CachePadded ===

impl<T> CachePadded<T> {
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for CachePadded<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for CachePadded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
