//! Text formatting utilities.
pub use core::fmt::*;
pub use tracing::field::{debug, display};

/// A wrapper type that formats the wrapped value using a provided function.
pub struct FormatWith<T, F = fn(&T, &mut Formatter<'_>) -> Result>
where
    F: Fn(&T, &mut Formatter<'_>) -> Result,
{
    value: T,
    fmt: F,
}

#[derive(Debug)]
pub struct WithIndent<'writer, W> {
    writer: &'writer mut W,
    indent: usize,
}
pub trait WriteExt: Write {
    fn with_indent(&mut self, indent: usize) -> WithIndent<'_, Self>
    where
        Self: Sized,
    {
        WithIndent {
            writer: self,
            indent,
        }
    }
}

pub struct FmtOption<'a, T> {
    opt: Option<&'a T>,
    or_else: &'a str,
}

/// Format the provided value using its [`core::fmt::Pointer`] implementation.
///
/// # Examples
/// ```
/// use mycelium_util::fmt;
/// use tracing::debug;
///
/// let something = "a string";
/// let some_ref = &something;
///
/// debug!(x = ?some_ref);            // will format the pointed value ("a string")
/// debug!(x = fmt::ptr(some_ref)); // will format the address.
///
/// ```
#[inline]
pub fn ptr<T: Pointer>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: Pointer::fmt,
    })
}

/// Format the provided value using its [`core::fmt::LowerHex`] implementation.
///
/// # Examples
/// ```
/// use mycelium_util::fmt;
/// use tracing::debug;
///
/// let n = 0xf00;
///
/// debug!(some_number = ?n);            // will be formatted as "some_number=3840"
/// debug!(some_number = fmt::hex(n)); //will be formatted as "some_number=0xf00"
///
/// ```
#[inline]
pub fn hex<T: LowerHex>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut Formatter<'_>| write!(f, "{:#x}", value),
    })
}

/// Format the provided value using its [`core::fmt::Binary`] implementation.
///
/// # Examples
/// ```
/// use mycelium_util::fmt;
/// use tracing::debug;
///
/// let n = 42;
///
/// debug!(some_number = ?n);            // will be formatted as "some_number=42"
/// debug!(some_number = fmt::bin(n)); //will be formatted as "some_number=0b101010"
///
/// ```
#[inline]
pub fn bin<T: Binary>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut Formatter<'_>| write!(f, "{:#b}", value),
    })
}

/// Format the provided value using its [`core::fmt::Debug`] implementation,
/// with "alternate mode" set
///
/// # Examples
/// ```
/// use mycelium_util::fmt;
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
/// debug!(something = fmt::alt(&thing)); // will be formatted with newlines and indentation
///
/// ```
#[inline]
pub fn alt<T: Debug>(value: T) -> tracing::field::DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut Formatter<'_>| write!(f, "{:#?}", value),
    })
}

pub fn opt<T>(value: &Option<T>) -> FmtOption<'_, T> {
    FmtOption::new(value)
}

impl<T, F> Debug for FormatWith<T, F>
where
    F: Fn(&T, &mut Formatter<'_>) -> Result,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        (self.fmt)(&self.value, f)
    }
}

impl<'a, W: Write> Write for WithIndent<'a, W> {
    fn write_str(&mut self, mut s: &str) -> Result {
        while !s.is_empty() {
            let (split, nl) = match s.find('\n') {
                Some(pos) => (pos + 1, true),
                None => (s.len(), false),
            };
            self.writer.write_str(&s[..split])?;
            if nl {
                for _ in 0..self.indent {
                    self.writer.write_char(' ')?;
                }
            }
            s = &s[split..];
        }

        Ok(())
    }
}

impl<W> WriteExt for W where W: Write {}

// === impl FmtOption ===

impl<'a, T> FmtOption<'a, T> {
    pub fn new(opt: &'a Option<T>) -> Self {
        Self {
            opt: opt.as_ref(),
            or_else: "",
        }
    }

    pub fn or_else(self, or_else: &'a str) -> Self {
        Self { or_else, ..self }
    }
}

impl<T: Debug> Debug for FmtOption<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: Display> Display for FmtOption<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}
