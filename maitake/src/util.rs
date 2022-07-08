use core::ptr::NonNull;

macro_rules! event {
    ($level:expr, $($arg:tt)+) => {
        {
            #[cfg(any(feature = "tracing-01", loom))]
            {
                use tracing_01::Level;
                tracing_01::event!($level, $($arg)+)
            }

            #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
            {
                use tracing_02::Level;
                tracing_02::event!($level, $($arg)+)
            }
        }
    };
}

macro_rules! in_span {
    ($level:expr, $($arg:tt)+) => {
        #[cfg(any(feature = "tracing-01", loom))]
        let _span_01 = {
            use tracing_01::Level;
            tracing_01::span!($level, $($arg)+).entered()
        };
        #[cfg(any(feature = "tracing-02", all(test, not(loom))))]
        let _span_02 = {
            use tracing_02::Level;
            tracing_02::span!($level,  $($arg)+).entered()
        };
    };
}

macro_rules! trace {
    ($($arg:tt)+) => {
        event!(Level::TRACE, $($arg)+)
    };
}

macro_rules! debug {
    ($($arg:tt)+) => {
        event!(Level::DEBUG, $($arg)+)
    };
}

#[allow(unused_macros)]
macro_rules! info {
    ($($arg:tt)+) => {
        event!(Level::INFO, $($arg)+)
    };
}

#[allow(unused_macros)]
macro_rules! in_trace_span {
    ($($arg:tt)+) => {
        in_span!(Level::TRACE, $($arg)+)
    };
}

macro_rules! in_debug_span {
    ($($arg:tt)+) => {
        in_span!(Level::DEBUG, $($arg)+)
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

macro_rules! fmt_bits {
    ($self: expr, $f: expr, $has_states: ident, $($name: ident),+) => {
        $(
            if $self.is(Self::$name) {
                if $has_states {
                    $f.write_str(" | ")?;
                }
                $f.write_str(stringify!($name))?;
                $has_states = true;
            }
        )+

    };
}

macro_rules! feature {
    (
        #![$meta:meta]
        $($item:item)*
    ) => {
        $(
            #[cfg($meta)]
            #[cfg_attr(docsrs, doc(cfg($meta)))]
            $item
        )*
    }
}

macro_rules! loom_const_fn {
    (
        $(#[$meta:meta])*
        $vis:vis fn $name:ident($($arg:ident: $T:ty),*) -> $Ret:ty $body:block
    ) => {
        $(#[$meta])*
        #[cfg(not(loom))]
        $vis const fn $name($($arg: $T),*) -> $Ret $body

        $(#[$meta])*
        #[cfg(loom)]
        $vis fn $name($($arg: $T),*) -> $Ret $body
    }
}

/// Helper to construct a `NonNull<T>` from a raw pointer to `T`, with null
/// checks elided in release mode.
#[cfg(debug_assertions)]
#[track_caller]
#[inline(always)]
pub(crate) unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new(ptr).expect(
        "/!\\ constructed a `NonNull` from a null pointer! /!\\ \n\
        in release mode, this would have called `NonNull::new_unchecked`, \
        violating the `NonNull` invariant! this is a bug in `cordyceps!`.",
    )
}

/// Helper to construct a `NonNull<T>` from a raw pointer to `T`, with null
/// checks elided in release mode.
///
/// This is the release mode version.
#[cfg(not(debug_assertions))]
#[inline(always)]
unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new_unchecked(ptr)
}

#[cfg(all(test, not(loom)))]
pub(crate) fn trace_init() {
    use tracing_subscriber::filter::LevelFilter;
    let _ = tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_test_writer()
        .try_init();
}
