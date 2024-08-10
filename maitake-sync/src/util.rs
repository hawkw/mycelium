//! Reusable utilities for synchronization primitives.
//!
//! This module contains utility code used in the implementation of the
//! synchronization primitives provided by `maitake-sync`. To enable code reuse,
//! some of these utilities are exposed as public APIs in this module, so that
//! projects depending on `maitake-sync` can use them as well.
//!
//! This module exposes the following APIs:
//!
//! - [`Backoff`]: exponential backoff for spin loops
//! - [`CachePadded`]: pads and aligns a value to the size of a cache line

#[cfg(any(test, feature = "tracing", loom))]
macro_rules! trace {
    ($($t:tt)*) => { tracing::trace!($($t)*) }
}

#[cfg(not(any(test, feature = "tracing", loom)))]
macro_rules! trace {
    ($($t:tt)*) => {};
}

#[cfg(all(not(test), not(all(maitake_ultraverbose, feature = "tracing"))))]
macro_rules! test_dbg {
    ($e:expr) => {
        $e
    };
}

#[cfg(any(test, all(maitake_ultraverbose, feature = "tracing")))]
macro_rules! test_dbg {
    ($e:expr) => {
        match $e {
            e => {
                tracing::debug!(
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

#[cfg(all(not(test), not(all(maitake_ultraverbose, feature = "tracing"))))]
macro_rules! test_debug {
    ($($t:tt)*) => {};
}

#[cfg(any(test, all(maitake_ultraverbose, feature = "tracing")))]
macro_rules! test_debug {
    ($($t:tt)*) => { tracing::debug!($($t)*) }
}

#[cfg(all(not(test), not(all(maitake_ultraverbose, feature = "tracing"))))]
macro_rules! test_trace {
    ($($t:tt)*) => {};
}

#[cfg(any(test, all(maitake_ultraverbose, feature = "tracing")))]
macro_rules! test_trace {
    ($($t:tt)*) => { tracing::trace!($($t)*) }
}

#[cfg(all(not(test), not(all(maitake_ultraverbose, feature = "tracing"))))]
macro_rules! enter_test_debug_span {
    ($($args:tt)+) => {};
}

#[cfg(any(test, all(maitake_ultraverbose, feature = "tracing")))]
macro_rules! enter_test_debug_span {
    ($($args:tt)+) => {
        let _span = tracing::debug_span!($($args)+).entered();
    };
}

macro_rules! fmt_bits {
    ($self: expr, $f: expr, $has_states: ident, $($name: ident),+) => {
        $(
            if $self.contains(Self::$name) {
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
        $vis:vis unsafe fn $name:ident($($arg:ident: $T:ty),*) -> $Ret:ty $body:block
    ) => {
        $(#[$meta])*
        #[cfg(not(loom))]
        $vis const unsafe fn $name($($arg: $T),*) -> $Ret $body

        $(#[$meta])*
        #[cfg(loom)]
        $vis unsafe fn $name($($arg: $T),*) -> $Ret $body
    };
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

/// Indicates unreachable code that we are confident is *truly* unreachable.
///
/// This is essentially a compromise between `core::unreachable!()` and
/// `core::hint::unreachable_unchecked()`. In debug mode builds and in tests,
/// this expands to `unreachable!()`, causing a panic. However, in release mode
/// non-test builds, this expands to `unreachable_unchecked`. Thus, this is a
/// somewhat safer form of `unreachable_unchecked` that will allow cases where
/// `unreachable_unchecked` would be invalid to be detected early.
///
/// Nonetheless, this must still be used with caution! If code is not adequately
/// tested, it is entirely possible for the `unreachable_unchecked` to be
/// reached in a scenario that was not reachable in tests.
macro_rules! unreachable_unchecked {
    () => ({
        #[cfg(any(test, debug_assertions))]
        panic!(
            concat!(
                env!("CARGO_PKG_NAME"),
                "internal error: entered unreachable code \n",
                "/!\\ EXTREMELY SERIOUS WARNING: in release mode, this would have been\n",
                "    `unreachable_unchecked`! This could result in undefine behavior.\n",
                "    Please double- or triple-check any assumptions about code which\n,",
                "    could lead to this being triggered."
            ),
        );
        #[allow(unreachable_code)] // lol
        {
            core::hint::unreachable_unchecked();
        }
    });
    ($msg:expr) => ({
        unreachable_unchecked!("{}", $msg)
    });
    ($msg:expr,) => ({
        unreachable_unchecked!($msg)
    });
    ($fmt:expr, $($arg:tt)*) => ({
        #[cfg(any(test, debug_assertions))]
        panic!(
            concat!(
                env!("CARGO_PKG_NAME"),
                "internal error: entered unreachable code: ",
                $fmt,
                "\n/!\\ EXTREMELY SERIOUS WARNING: in release mode, this would have been \n\
                \x32   `unreachable_unchecked`! This could result in undefine behavior. \n\
                \x32   Please double- or triple-check any assumptions about code which \n\
                \x32   could lead to this being triggered."
            ),
            $($arg)*
        );
        #[allow(unreachable_code)] // lol
        {
            core::hint::unreachable_unchecked();
        }
    });
}

mod backoff;
mod cache_pad;
pub(crate) mod fmt;
mod maybe_uninit;
mod wake_batch;

#[cfg(all(test, not(loom)))]
pub(crate) use self::test::trace_init;
pub use self::{backoff::Backoff, cache_pad::CachePadded};
pub(crate) use self::{maybe_uninit::CheckedMaybeUninit, wake_batch::WakeBatch};

#[cfg(test)]
pub(crate) mod test {
    /// A guard that represents the tracing default subscriber guard
    ///
    /// *should* be held until the end of the test, to ensure that tracing messages
    /// actually make it to the fmt subscriber for the entire test.
    ///
    /// Exists to abstract over tracing 01/02 guard type differences.
    #[must_use]
    #[cfg(all(test, not(loom)))]
    pub struct TestGuard {
        _x1: tracing::subscriber::DefaultGuard,
    }

    /// Initialize tracing with a default filter directive
    ///
    /// Returns a [TestGuard] that must be held for the duration of test to ensure
    /// tracing messages are correctly output

    #[cfg(all(test, not(loom)))]
    pub(crate) fn trace_init() -> TestGuard {
        trace_init_with_default("maitake=debug,cordyceps=debug")
    }

    /// Initialize tracing with the given filter directive
    ///
    /// Returns a [TestGuard] that must be held for the duration of test to ensure
    /// tracing messages are correctly output
    #[cfg(all(test, not(loom)))]
    pub(crate) fn trace_init_with_default(default: &str) -> TestGuard {
        use tracing_subscriber::{
            filter::{EnvFilter, LevelFilter},
            util::SubscriberInitExt,
        };
        const ENV: &str = if cfg!(loom) { "LOOM_LOG" } else { "RUST_LOG" };

        let env = std::env::var(ENV).unwrap_or_default();
        let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
        let filter = if env.is_empty() {
            builder
                .parse(default)
                .unwrap()
                // enable "loom=info" if using the default, so that we get
                // loom's thread number and iteration count traces.
                .add_directive("loom=info".parse().unwrap())
        } else {
            builder.parse_lossy(env)
        };
        let collector = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .without_time()
            .finish();

        TestGuard {
            _x1: collector.set_default(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn assert_send<T: Send>() {}

    #[allow(dead_code)]
    pub(crate) fn assert_sync<T: Sync>() {}
    pub(crate) fn assert_send_sync<T: Send + Sync>() {}
}
