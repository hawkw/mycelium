#![cfg_attr(not(test), allow(dead_code, unused_macros))]

use mycelium_util::fmt;

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

#[cfg(test)]
macro_rules! info {
    ($($arg:tt)+) => {
        #[cfg(any(feature = "tracing-01", loom))]
        {
            tracing_01::info!($($arg)+)
        }

        #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
        {
            tracing_02::info!($($arg)+)
        }
    };
}

macro_rules! trace_span {
    ($($arg:tt)+) => {
        span!(Level::TRACE, $($arg)+)
    };
}

#[allow(unused_macros)]
macro_rules! debug_span {
    ($($arg:tt)+) => {
        span!(Level::DEBUG, $($arg)+)
    };
}

#[cfg(all(not(test), not(maitake_ultraverbose)))]
macro_rules! test_dbg {
    ($e:expr) => {
        $e
    };
}

#[cfg(any(test, maitake_ultraverbose))]
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

#[cfg(all(not(test), not(maitake_ultraverbose)))]
macro_rules! test_debug {
    ($($args:tt)+) => {};
}

#[cfg(any(test, maitake_ultraverbose))]
macro_rules! test_debug {
    ($($args:tt)+) => {
        debug!($($args)+);
    };
}

#[cfg(all(not(test), not(maitake_ultraverbose)))]
macro_rules! test_trace {
    ($($args:tt)+) => {};
}

#[cfg(any(test, maitake_ultraverbose))]
macro_rules! test_trace {
    ($($args:tt)+) => {
        trace!($($args)+);
    };
}

#[derive(Clone)]
pub(crate) struct Span {
    #[cfg(any(feature = "tracing-01", loom))]
    pub(crate) span_01: tracing_01::Span,
    #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
    pub(crate) span_02: tracing_02::Span,
}

impl Span {
    #[inline(always)]
    pub(crate) const fn none() -> Self {
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

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const TRACING_01_FIELD: &str = "tracing_01";
        const TRACING_02_FIELD: &str = "tracing_02";

        let mut s = f.debug_struct("Span");

        #[cfg(any(feature = "tracing-01", loom))]
        if let Some(id) = self.span_01.id() {
            s.field(TRACING_01_FIELD, &id.into_u64());
        } else {
            s.field(TRACING_02_FIELD, &fmt::display("<none>"));
        }

        #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
        if let Some(id) = self.span_02.id() {
            s.field(TRACING_02_FIELD, &id.into_u64());
        } else {
            s.field(TRACING_01_FIELD, &fmt::display("<none>"));
        }

        s.finish()
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
