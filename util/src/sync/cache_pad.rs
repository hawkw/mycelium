use core::{
    fmt,
    ops::{Deref, DerefMut},
};

pub use self::inner::CachePadded;

/// When configured not to pad to cache alignment, just provide a no-op wrapper struct
/// This feature is useful for platforms with no data cache, such as many Cortex-M
/// targets.
#[cfg(feature = "no-cache-pad")]
mod inner {
    /// Aligns the wrapped value to the size of a cache line.
    ///
    /// This is used to avoid [false sharing] for values that may be
    /// accessed concurrently.
    ///
    /// # Size/Alignment
    ///
    /// The size and alignment of this type depends on the target architecture,
    /// and on whether or not the `no-cache-pad` feature flag is enabled.
    ///
    /// When the `no-cache-pad` crate feature flag is enabled, this is simply a
    /// no-op wrapper struct. This is intended for use on useful for platforms
    /// with no data cache, such as many Cortex-M targets.
    ///
    /// In other cases, this type is always aligned to the size of a cache line,
    /// based on the target architecture. On `x86_64`/`aarch64`, a cache line is
    /// 128 bytes. On all other targets, a cache line is assumed to 64 bytes
    /// long. This type's size will always be a multiple of the cache line size;
    /// if the wrapped type is longer than the alignment of a cache line, then
    /// this type will be padded to multiple cache lines.
    ///
    /// [false sharing]: https://en.wikipedia.org/wiki/False_sharing
    #[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
    pub struct CachePadded<T>(pub(super) T);
}

/// When not inhibited, determine cache alignment based on target architecture.
/// Align to 128 bytes on 64-bit x86/ARM targets, otherwise align to 64 bytes.
#[cfg(not(feature = "no-cache-pad"))]
mod inner {
    /// Aligns the wrapped value to the size of a cache line.
    ///
    /// This is used to avoid [false sharing] for values that may be
    /// accessed concurrently.
    ///
    /// # Size/Alignment
    ///
    /// The size and alignment of this type depends on the target architecture,
    /// and on whether or not the `no-cache-pad` feature flag is enabled.
    ///
    /// When the `no-cache-pad` crate feature flag is enabled, this is simply a
    /// no-op wrapper struct. This is intended for use on useful for platforms
    /// with no data cache, such as many Cortex-M targets.
    ///
    /// In other cases, this type is always aligned to the size of a cache line,
    /// based on the target architecture. On `x86_64`/`aarch64`, a cache line is
    /// 128 bytes. On all other targets, a cache line is assumed to 64 bytes
    /// long. This type's size will always be a multiple of the cache line size;
    /// if the wrapped type is longer than the alignment of a cache line, then
    /// this type will be padded to multiple cache lines.
    ///
    /// [false sharing]: https://en.wikipedia.org/wiki/False_sharing
    #[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), repr(align(128)))]
    #[cfg_attr(
        not(any(target_arch = "x86_64", target_arch = "aarch64")),
        repr(align(64))
    )]
    #[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
    pub struct CachePadded<T>(pub(super) T);
}

// === impl CachePadded ===

impl<T> CachePadded<T> {
    /// Pads `value` to the length of a cache line.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Unwraps the inner value and returns it.
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
