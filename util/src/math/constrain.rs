use core::fmt;
use core::ops::RangeBounds;

/// Extension trait that allows constraining ordered values within a range.
pub trait Constrain<T>: RangeBounds<T> {
    /// Constrain `value` to be within the range described by `self`, returning
    /// an error if it is not within the range.
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(U)` if `value` is contained within the range described by `self`.
    /// - [`Err`]`(`[`OutOfRange`]`<Self, U>)` if `value` is outside of the
    ///   range described by `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mycelium_util::math::Constrain;
    ///
    /// assert!((0..10).constrain(10).is_err());
    /// assert!((0..10).constrain(9).is_ok());
    /// assert!((0..=10).constrain(10).is_ok());
    /// assert!((..10).constrain(9).is_ok());
    /// ```
    fn constrain<U>(self, value: U) -> Result<U, OutOfRange<Self, U>>
    where
        T: PartialOrd<U>,
        U: PartialOrd<T>,
        Self: Sized,
    {
        if self.contains(&value) {
            return Ok(value);
        }

        Err(OutOfRange {
            value,
            range: self,
            message: "value",
        })
    }

    /// Constrain `value` to be within the range described by `self`, returning
    /// an error with a message describing the constrained value if it is not
    /// within the range.
    ///
    /// This is the same as [`Constrain::constrain`], but the returned
    /// [`OutOfRange`] error's [`fmt::Display`] output will say "{message}
    /// {value} must be within the range ..." rather than "value {value} must be
    /// within ...".
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(U)` if `value` is contained within the range described by `self`.
    /// - [`Err`]`(`[`OutOfRange`]`<Self, U>)` if `value` is outside of the
    ///   range described by `self`.
    fn constrain_named<U>(self, value: U, message: &'static str) -> Result<U, OutOfRange<Self, U>>
    where
        T: PartialOrd<U>,
        U: PartialOrd<T>,
        Self: Sized,
    {
        self.constrain(value).map_err(|e| e.with_message(message))
    }
}

impl<R, T> Constrain<T> for R where R: RangeBounds<T> {}

/// An error indicating that a numeric value fell outside of a specified range.
///
/// This is returned by the [`Constrain::constrain`] and
/// [`Constrain::constrain_named`] methods.
#[derive(Copy, Clone, Debug)]
pub struct OutOfRange<R, T> {
    value: T,
    range: R,
    message: &'static str,
}

impl<R, T> OutOfRange<R, T> {
    /// Borrows the value that was out of range.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Consumes `self`, returning the value that was out of range.
    pub fn into_value(self) -> T {
        self.value
    }

    /// Borrows the range that the value was expected to be in.
    pub fn range(&self) -> &R {
        &self.range
    }

    /// Consumes `self`, returning the range that the value was expected to be in.
    pub fn into_range(self) -> R {
        self.range
    }

    fn with_message(self, message: &'static str) -> Self {
        Self { message, ..self }
    }
}

impl<R: fmt::Debug, T: fmt::Debug> fmt::Display for OutOfRange<R, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            value,
            message,
            range,
        } = self;
        write!(f, "{message} {value:?} must be within the range {range:?}")
    }
}
