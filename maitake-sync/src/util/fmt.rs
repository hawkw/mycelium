pub use core::fmt::*;

/// A wrapper type that formats the wrapped value using a provided function.
///
/// This is used to implement the `ptr` and `display` util functions.
pub(crate) struct FormatWith<T, F = fn(&T, &mut Formatter<'_>) -> Result>
where
    F: Fn(&T, &mut Formatter<'_>) -> Result,
{
    value: T,
    fmt: F,
}

#[derive(Clone)]
pub(crate) struct FmtOption<'a, T> {
    opt: Option<&'a T>,
    or_else: &'a str,
}

// === impl FormatWith ===

#[cfg(any(test, feature = "tracing", loom))]
#[inline]
#[must_use]
pub(crate) fn ptr<T: Pointer>(value: T) -> FormatWith<T> {
    FormatWith {
        value,
        fmt: Pointer::fmt,
    }
}

#[inline]
#[must_use]
pub(crate) fn display<T: Display>(value: T) -> FormatWith<T> {
    FormatWith {
        value,
        fmt: Display::fmt,
    }
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

// === impl FmtOption ====

#[must_use]
#[inline]
pub(crate) const fn opt<T>(value: &Option<T>) -> FmtOption<'_, T> {
    FmtOption {
        opt: value.as_ref(),
        or_else: "",
    }
}

impl<'a, T> FmtOption<'a, T> {
    #[must_use]
    #[inline]
    pub(crate) fn or_else(self, or_else: &'a str) -> Self {
        Self { or_else, ..self }
    }
}

impl<T: Debug> Debug for FmtOption<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self.opt {
            Some(val) => Debug::fmt(val, f),
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
