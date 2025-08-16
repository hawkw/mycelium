//! Text formatting utilities.
pub use core::fmt::*;
pub use tracing::field::{debug, display, DebugValue};

/// A wrapper type that formats the wrapped value using a provided function.
///
/// This is used to implement the [`fmt::alt`](alt), [`fmt::bin`](bin),
/// [`fmt::hex`](hex), and [`fmt::ptr`](ptr) functions.
pub struct FormatWith<T, F = fn(&T, &mut Formatter<'_>) -> Result>
where
    F: Fn(&T, &mut Formatter<'_>) -> Result,
{
    value: T,
    fmt: F,
}

/// Wraps a type implementing [`core::fmt::Write`] so that every newline written to
/// that writer is indented a given amount.
#[derive(Debug)]
pub struct WithIndent<'writer, W> {
    writer: &'writer mut W,
    indent: usize,
}

/// Extension trait adding additional methods to types implementing [`core::fmt::Write`].
pub trait WriteExt: Write {
    /// Wraps `self` in a [`WithIndent`] writer that indents every new line
    /// that's written to it by `indent` spaces.
    #[must_use]
    #[inline]
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

/// A utility to assist with formatting [`Option`] values.
///
/// This wraps a reference to an [`Option`] value, and implements formatting
/// traits by formatting the `Option`'s contents if it is [`Some`]. If the
/// `Option` is [`None`], the formatting trait implementations emit no text by
/// default, or a string provided using the [`or_else`](Self::or_else) method.
///
/// A `FmtOption` will implement the [`core::fmt::Display`],
/// [`core::fmt::Debug`], [`core::fmt::Binary`], [`core::fmt::UpperHex`],
/// [`core::fmt::LowerHex`], and [`core::fmt::Pointer`] formatting traits, if
/// the inner type implements the corresponding trait.
///
/// The [`fmt::opt`](opt) method can be used as shorthand to borrow an `Option`
/// as a `FmtOption`.
///
/// # Examples
///
/// Formatting a [`Some`] value emits that value's [`Debug`] and [`Display`] output:
///
/// ```
/// use mycelium_util::fmt;
///
/// let value = Some("hello world");
/// let debug = format!("{:?}", fmt::opt(&value));
/// assert_eq!(debug, "\"hello world\"");
///
/// let display = format!("{}", fmt::opt(&value));
/// assert_eq!(display, "hello world");
/// ```
///
/// Formatting a [`None`] value generates no text by default:
///
/// ```
/// use mycelium_util::fmt;
///
/// let value: Option<&str> = None;
/// let debug = format!("{:?}", fmt::opt(&value));
/// assert_eq!(debug, "");
///
/// let display = format!("{}", fmt::opt(&value));
/// assert_eq!(display, "");
/// ```
///
/// The [`or_else`](Self::or_else) method can be used to customize the text that
/// is emitted when the value is [`None`]:
///
/// ```
/// use mycelium_util::fmt;
/// use core::ptr::NonNull;
///
/// let value: Option<NonNull<u8>> = None;
/// let debug = format!("{:?}", fmt::opt(&value).or_else("null"));
/// assert_eq!(debug, "null");
/// ```
#[derive(Clone)]
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
#[must_use]
pub fn ptr<T: Pointer>(value: T) -> DebugValue<FormatWith<T>> {
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
#[must_use]
pub fn hex<T: LowerHex>(value: T) -> DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut Formatter<'_>| write!(f, "{value:#x}"),
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
#[must_use]
#[inline]
pub fn bin<T: Binary>(value: T) -> DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut Formatter<'_>| write!(f, "{value:#b}"),
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
#[must_use]
#[inline]
pub fn alt<T: Debug>(value: T) -> DebugValue<FormatWith<T>> {
    tracing::field::debug(FormatWith {
        value,
        fmt: |value, f: &mut Formatter<'_>| write!(f, "{value:#?}"),
    })
}

/// Borrows an [`Option`] as a [`FmtOption`] that formats the inner value if
/// the [`Option`] is [`Some`], or emits a customizable string if the [`Option`]
/// is [`None`].
///
/// # Examples
///
/// Formatting a [`Some`] value emits that value's [`Debug`] and [`Display`] output:
///
/// ```
/// use mycelium_util::fmt;
///
/// let value = Some("hello world");
/// let debug = format!("{:?}", fmt::opt(&value));
/// assert_eq!(debug, "\"hello world\"");
///
/// let display = format!("{}", fmt::opt(&value));
/// assert_eq!(display, "hello world");
/// ```
///
/// Formatting a [`None`] value generates no text by default:
///
/// ```
/// use mycelium_util::fmt;
///
/// let value: Option<&str> = None;
/// let debug = format!("{:?}", fmt::opt(&value));
/// assert_eq!(debug, "");
///
/// let display = format!("{}", fmt::opt(&value));
/// assert_eq!(display, "");
/// ```
///
/// The [`or_else`](FmtOption::or_else) method can be used to customize the text that
/// is emitted when the value is [`None`]:
///
/// ```
/// use mycelium_util::fmt;
/// use core::ptr::NonNull;
///
/// let value: Option<NonNull<u8>> = None;
/// let debug = format!("{:?}", fmt::opt(&value).or_else("null"));
/// assert_eq!(debug, "null");
/// ```
#[must_use]
#[inline]
pub const fn opt<T>(value: &Option<T>) -> FmtOption<'_, T> {
    FmtOption::new(value)
}

/// Formats a list of `F`-typed values to the provided `writer`, delimited with commas.
pub fn comma_delimited<F: Display>(
    mut writer: impl Write,
    values: impl IntoIterator<Item = F>,
) -> Result {
    let mut values = values.into_iter();
    if let Some(value) = values.next() {
        write!(writer, "{value}")?;
        for value in values {
            write!(writer, ", {value}")?;
        }
    }

    Ok(())
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

impl<W: Write> Write for WithIndent<'_, W> {
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
    /// Returns a new `FmtOption` that formats the provided [`Option`] value.
    ///
    /// The [`fmt::opt`](opt) function can be used as shorthand for this.
    #[must_use]
    #[inline]
    pub const fn new(opt: &'a Option<T>) -> Self {
        Self {
            opt: opt.as_ref(),
            or_else: "",
        }
    }

    /// Set the text to emit when the value is [`None`].
    ///
    /// # Examples
    ///
    /// If the value is [`None`], the `or_else` text is emitted:
    ///
    /// ```
    /// use mycelium_util::fmt;
    /// use core::ptr::NonNull;
    ///
    /// let value: Option<NonNull<u8>> = None;
    /// let debug = format!("{:?}", fmt::opt(&value).or_else("null"));
    /// assert_eq!(debug, "null");
    /// ```
    ///
    /// If the value is [`Some`], this function does nothing:
    ///
    /// ```
    /// # use mycelium_util::fmt;
    /// # use core::ptr::NonNull;
    /// let value = Some("hello world");
    /// let debug = format!("{}", fmt::opt(&value).or_else("no string"));
    /// assert_eq!(debug, "hello world");
    /// ```
    #[must_use]
    #[inline]
    pub fn or_else(self, or_else: &'a str) -> Self {
        Self { or_else, ..self }
    }
}

impl<T: Debug> Debug for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: Display> Display for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: Binary> Binary for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: UpperHex> UpperHex for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: LowerHex> LowerHex for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}

impl<T: Pointer> Pointer for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => val.fmt(f),
            None => f.write_str(self.or_else),
        }
    }
}
