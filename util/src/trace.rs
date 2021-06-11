//! Tracing utilities.
use core::fmt;

/// A wrapper type that formats the wrapped value using a provided function.
pub struct FormatWith<T, F = fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result>
where
    F: Fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result,
{
    value: T,
    fmt: F,
}

/// Format the provided value using its [`core::fmt::Pointer`] implementation.
///
/// # Examples
/// ```
/// panic!("lol");
/// use mycelium_util::trace;
/// use tracing::debug;
///
/// let something = "a string";
/// let some_ref = &something;
///
/// debug!(x = ?some_ref);            // will format the pointed value ("a string")
/// debug!(x = trace::ptr(some_ref)); // will format the address.
///
/// ```
#[inline]
pub fn ptr<T: fmt::Pointer>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: fmt::Pointer::fmt,
    })
}

/// Format the provided value using its [`core::fmt::LowerHex`] implementation.
///
/// # Examples
/// ```
/// use mycelium_util::trace;
/// use tracing::debug;
///
/// let n = 0xf00;
///
/// debug!(some_number = ?n);            // will be formatted as "some_number=3840"
/// debug!(some_number = trace::hex(n)); //will be formatted as "some_number=0xf00"
///
/// ```
#[inline]
pub fn hex<T: fmt::LowerHex>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut fmt::Formatter<'_>| write!(f, "{:#x}", value),
    })
}

/// Format the provided value using its [`core::fmt::Binary`] implementation.
///
/// # Examples
/// ```
/// use mycelium_util::trace;
/// use tracing::debug;
///
/// let n = 42;
///
/// debug!(some_number = ?n);            // will be formatted as "some_number=42"
/// debug!(some_number = trace::bin(n)); //will be formatted as "some_number=0b101010"
///
/// ```
#[inline]
pub fn bin<T: fmt::Binary>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut fmt::Formatter<'_>| write!(f, "{:#b}", value),
    })
}

/// Format the provided value using its [`core::fmt::Debug`] implementation,
/// with "alternate mode" set
///
/// # Examples
/// ```
/// use mycelium_util::trace;
/// use tracing::debug;
///
/// #[derive(Debug)]
/// struct Thing {
///     question: &'static str,
///     answer: usize,
/// }
/// let thing = Thing {
///     question: "life, the universe, and everything",
///     answer: 42,
/// };
///
/// debug!(something = ?thing);             // will be formatted on the current line
/// debug!(something = trace::alt(&thing)); // will be formatted with newlines and indentation
///
/// ```
#[inline]
pub fn alt<T: fmt::Debug>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut fmt::Formatter<'_>| write!(f, "{:#?}", value),
    })
}

impl<T, F> fmt::Debug for FormatWith<T, F>
where
    F: Fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.fmt)(&self.value, f)
    }
}
