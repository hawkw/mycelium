use core::{
    fmt,
    ops::{Deref, DerefMut},
};

#[cfg(target_has_atomic = "ptr")]
macro_rules! feature {
    (
        #![$meta:meta]
        $($item:item)*
    ) => {
        $(
            #[cfg($meta)]
            $item
        )*
    }
}

macro_rules! test_trace {
    ($($tt:tt)*) => {
        #[cfg(test)]
        tracing::trace!($($tt)*)
    }
}

/// An exponential backoff for spin loops
#[cfg(target_has_atomic = "ptr")]
#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    exp: u8,
    max: u8,
}

pub(crate) use cache_pad::CachePadded;

/// When configured not to pad to cache alignment, just provide a no-op wrapper struct
/// This feature is useful for platforms with no data cache, such as many Cortex-M
/// targets.
#[cfg(feature = "no-cache-pad")]
mod cache_pad {
    #[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
    pub(crate) struct CachePadded<T>(pub(crate) T);
}

/// When not inhibited, determine cache alignment based on target architecture.
/// Align to 128 bytes on 64-bit x86/ARM targets, otherwise align to 64 bytes.
#[cfg(not(feature = "no-cache-pad"))]
mod cache_pad {
    #[cfg_attr(not(target_has_atomic = "ptr"), allow(dead_code))]
    #[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), repr(align(128)))]
    #[cfg_attr(
        not(any(target_arch = "x86_64", target_arch = "aarch64")),
        repr(align(64))
    )]
    #[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
    pub(crate) struct CachePadded<T>(pub(crate) T);
}

pub(crate) struct FmtOption<'a, T> {
    opt: Option<&'a T>,
    or_else: &'a str,
}

// === impl Backoff ===

#[cfg(target_has_atomic = "ptr")]
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
    #[cfg(target_has_atomic = "ptr")]
    #[inline(always)]
    pub(crate) fn spin(&mut self) {
        use crate::loom::hint;

        // Issue 2^exp pause instructions.
        for _ in 0..(1 << self.exp) {
            hint::spin_loop();
        }

        if self.exp < self.max {
            self.exp += 1
        }
    }
}

#[cfg(target_has_atomic = "ptr")]
impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

// === impl CachePadded ===

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

// === impl FmtOption ===

impl<'a, T> FmtOption<'a, T> {
    pub(crate) fn new(opt: &'a Option<T>) -> Self {
        Self {
            opt: opt.as_ref(),
            or_else: "None",
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for FmtOption<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: fmt::Display> fmt::Display for FmtOption<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

#[cfg(test)]
pub(crate) fn assert_send_sync<T: Send + Sync>() {}
