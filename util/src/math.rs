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
pub trait Logarithm: Sized {
    /// Returns `ceiling(log2(self))`.
    fn log2_ceil(self) -> Self;

    /// Returns `log2(self)`.
    fn log2(self) -> Self;

    /// Returns the integer logarithm base `base` of `self`, or `None` if it is
    /// not possible  to take the log base `base` of `self`.
    fn checked_log(self, base: Self) -> Option<Self>;

    /// Returns the integer logarithm base `base` of `self`.
    fn log(self, base: Self) -> Self;
}

impl Logarithm for usize {
    #[inline(always)]
    #[must_use = "this returns the result of the operation, \
                    without modifying the original"]
    fn log2_ceil(self) -> usize {
        usize_const_log2_ceil(self)
    }

    #[inline(always)]
    #[must_use = "this returns the result of the operation, \
                    without modifying the original"]
    fn log2(self) -> usize {
        usize_const_log2(self)
    }

    #[inline(always)]
    #[must_use = "this returns the result of the operation, \
                    without modifying the original"]
    fn checked_log(self, base: usize) -> Option<Self> {
        usize_const_checked_log(self, base)
    }

    #[inline(always)]
    #[must_use = "this returns the result of the operation, \
                    without modifying the original"]
    fn log(self, base: usize) -> Self {
        match self.checked_log(base) {
            Some(log) => log,
            None => panic!("cannot take log base {base} of {self}"),
        }
    }
}

/// Returns `ceiling(log2(n))`.
///
/// This is exposed in addition to the [`Logarithm`] extension trait because it
/// is a  `const fn`, while trait methods cannot be `const fn`s.
#[inline(always)]
#[must_use = "this returns the result of the operation, \
                without modifying the original"]
pub const fn usize_const_log2_ceil(n: usize) -> usize {
    n.next_power_of_two().trailing_zeros() as usize
}

/// Returns `log2(n)`.
///
/// This is exposed in addition to the [`Logarithm`] extension trait because it
/// is a
/// `const fn`, while trait methods cannot be `const fn`s.
#[inline(always)]
#[must_use = "this returns the result of the operation, \
                without modifying the original"]
pub const fn usize_const_log2(n: usize) -> usize {
    (usize::BITS - 1) as usize - n.leading_zeros() as usize
}

/// Returns the `base` logarithm of `n`.
///
/// This is exposed in addition to the [`Logarithm`] extension trait because it
/// is a `const fn`, while trait methods cannot be `const fn`s.
#[inline(always)]
#[must_use = "this returns the result of the operation, \
                without modifying the original"]
pub const fn usize_const_checked_log(mut n: usize, base: usize) -> Option<usize> {
    if n == 0 || base <= 1 {
        return None;
    }

    if base == 2 {
        return Some(usize_const_log2(n));
    }

    let mut log = 0;
    while n >= base {
        n /= base;
        log += 1;
    }
    Some(log)
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
