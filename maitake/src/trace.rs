#![cfg_attr(not(test), allow(dead_code, unused_macros))]

macro_rules! span {
    ($level:expr, $($arg:tt)+) => {
        crate::trace::Span {
            #[cfg(any(feature = "tracing-01", loom))]
            span_01: {
                use tracing_01::Level;
                tracing_01::span!($level, $($arg)+)
            },

            #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
            span_02: {
                use tracing_02::Level;
                tracing_02::span!($level, $($arg)+)
            }
        }
    }
}

macro_rules! trace {
    ($($arg:tt)+) => {
        #[cfg(any(feature = "tracing-01", loom))]
        {
            tracing_01::trace!($($arg)+)
        }

        #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
        {
            tracing_02::trace!($($arg)+)
        }
    };
}

macro_rules! debug {
    ($($arg:tt)+) => {
        #[cfg(any(feature = "tracing-01", loom))]
        {
            tracing_01::debug!($($arg)+)
        }

        #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
        {
            tracing_02::debug!($($arg)+)
        }
    };
}

macro_rules! trace_span {
    ($($arg:tt)+) => {
        span!(Level::TRACE, $($arg)+)
    };
}

macro_rules! debug_span {
    ($($arg:tt)+) => {
        span!(Level::DEBUG, $($arg)+)
    };
}

#[cfg(not(test))]
macro_rules! test_dbg {
    ($e:expr) => {
        $e
    };
}

#[cfg(test)]
macro_rules! test_dbg {
    ($e:expr) => {
        match $e {
            e => {
                debug!(
                    location = %core::panic::Location::caller(),
                    "{} = {:?}",
                    stringify!($e),
                    &e
                );
                e
            }
        }
    };
}

#[cfg(not(test))]
macro_rules! test_trace {
    ($($args:tt)+) => {};
}

#[cfg(test)]
macro_rules! test_trace {
    ($($args:tt)+) => {
        debug!(
            location = %core::panic::Location::caller(),
            $($args)+
        );
    };
}

#[derive(Clone, Debug)]
pub(crate) struct Span {
    #[cfg(any(feature = "tracing-01", loom))]
    pub(crate) span_01: tracing_01::Span,
    #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
    pub(crate) span_02: tracing_02::Span,
}

impl Span {
    #[inline(always)]
    pub(crate) fn none() -> Self {
        Span {
            #[cfg(any(feature = "tracing-01", loom))]
            span_01: tracing_01::Span::none(),
            #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
            span_02: tracing_02::Span::none(),
        }
    }

    #[inline]
    pub(crate) fn enter(&self) -> Entered<'_> {
        Entered {
            #[cfg(any(feature = "tracing-01", loom))]
            _enter_01: self.span_01.enter(),

            #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
            _enter_02: self.span_02.enter(),

            _p: core::marker::PhantomData,
        }
    }

    #[inline]
    pub(crate) fn entered(self) -> EnteredSpan {
        EnteredSpan {
            #[cfg(any(feature = "tracing-01", loom))]
            _enter_01: self.span_01.entered(),
            #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
            _enter_02: self.span_02.entered(),
        }
    }

    #[cfg(any(feature = "tracing-01", loom))]
    #[inline]
    pub(crate) fn tracing_01_id(&self) -> Option<core::num::NonZeroU64> {
        self.span_01.id().map(|id| id.into_non_zero_u64())
    }
}

#[derive(Debug)]
pub(crate) struct Entered<'span> {
    #[cfg(any(feature = "tracing-01", loom))]
    _enter_01: tracing_01::span::Entered<'span>,
    #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
    _enter_02: tracing_02::span::Entered<'span>,

    /// This is just there so that the `'span` lifetime is used even when both
    /// `tracing` features are disabled.
    _p: core::marker::PhantomData<&'span ()>,
}

#[derive(Debug)]
pub(crate) struct EnteredSpan {
    #[cfg(any(feature = "tracing-01", loom))]
    _enter_01: tracing_01::span::EnteredSpan,
    #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
    _enter_02: tracing_02::span::EnteredSpan,
}
