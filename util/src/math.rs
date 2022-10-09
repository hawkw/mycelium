//! Math utilities.

/// Variadic version of [`core::cmp::max`].
///
/// # Examples
///
/// ```
/// let max = mycelium_util::math::max![1, 2, 3, 4];
/// assert_eq!(max, 4);
/// ```
#[macro_export]
macro_rules! max {
    ($arg:expr) => { $arg };
    ($arg1:expr, $($arg:expr),+) => {
        core::cmp::max($arg1, $crate::max!( $($arg),+ ))
    };
}

/// Variadic version of [`core::cmp::min`].
///
/// # Examples
///
/// ```
/// let min = mycelium_util::math::min![1, 2, 3, 4];
/// assert_eq!(min, 1);
/// ```
#[macro_export]
macro_rules! min {
    ($arg:expr) => { $arg };
    ($arg1:expr, $($arg:expr),+) => {
        core::cmp::min($arg1, $crate::min!( $($arg),+ ))
    };
}

#[doc(inline)]
pub use max;

#[doc(inline)]
pub use min;

/// Extension trait adding logarithm methods to integers.
pub trait Log2 {
    /// Returns `ceiling(log2(self))`.
    fn log2_ceil(self) -> Self;
}

impl Log2 for usize {
    #[inline(always)]
    fn log2_ceil(self) -> usize {
        usize_const_log2_ceil(self)
    }
}

/// Returns `ceiling(log2(u))`.
///
/// This is exposed in addition to the [`Log2`] extension trait because it is a
/// `const fn`, while trait methods cannot be `const fn`s.
#[inline(always)]
#[must_use]
pub const fn usize_const_log2_ceil(u: usize) -> usize {
    u.next_power_of_two().trailing_zeros() as usize
}

#[cfg(all(test, not(loom)))]
#[test]
fn test_log2_ceil() {
    assert_eq!(0, 0.log2_ceil());
    assert_eq!(0, 1.log2_ceil());
    assert_eq!(1, 2.log2_ceil());
    assert_eq!(5, 32.log2_ceil());
    assert_eq!(10, 1024.log2_ceil());
}
