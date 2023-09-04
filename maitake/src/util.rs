use core::ptr::NonNull;

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
pub(crate) unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new_unchecked(ptr)
}

#[cfg(test)]
pub(crate) use self::test::trace_init;

pub(crate) fn expect_display<T, E: core::fmt::Display>(result: Result<T, E>, msg: &str) -> T {
    match result {
        Ok(t) => t,
        Err(error) => panic!("{msg}: {error}"),
    }
}

#[cfg(test)]
pub(crate) mod test {
    /// A guard that represents the tracing default subscriber guard
    ///
    /// *should* be held until the end of the test, to ensure that tracing messages
    /// actually make it to the fmt subscriber for the entire test.
    ///
    /// Exists to abstract over tracing 01/02 guard type differences.
    #[must_use]
    pub struct TestGuard {
        #[cfg(not(loom))]
        _x2: tracing_02::collect::DefaultGuard,
        #[cfg(loom)]
        _x1: tracing_01::subscriber::DefaultGuard,
    }

    /// Initialize tracing with a default filter directive
    ///
    /// Returns a [TestGuard] that must be held for the duration of test to ensure
    /// tracing messages are correctly output
    pub(crate) fn trace_init() -> TestGuard {
        trace_init_with_default("maitake=debug,cordyceps=debug")
    }

    /// Initialize tracing with the given filter directive
    ///
    /// Returns a [TestGuard] that must be held for the duration of test to ensure
    /// tracing messages are correctly output
    #[cfg(not(loom))]
    pub(crate) fn trace_init_with_default(default: &str) -> TestGuard {
        use tracing_subscriber::filter::{EnvFilter, LevelFilter};
        let env = std::env::var("RUST_LOG").unwrap_or_default();
        let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
        let filter = if env.is_empty() {
            builder.parse(default).unwrap()
        } else {
            builder.parse_lossy(env)
        };
        // enable traces from alloc leak checking.
        let filter = filter.add_directive("maitake::alloc=trace".parse().unwrap());
        let collector = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .without_time()
            .finish();

        TestGuard {
            _x2: tracing_02::collect::set_default(collector),
        }
    }

    /// Initialize tracing with the given filter directive
    ///
    /// Returns a [TestGuard] that must be held for the duration of test to ensure
    /// tracing messages are correctly output
    #[cfg(loom)]
    pub(crate) fn trace_init_with_default(default: &str) -> TestGuard {
        use tracing_subscriber_03::filter::{EnvFilter, LevelFilter};
        let env = std::env::var("LOOM_LOG").unwrap_or_default();
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
        let collector = tracing_subscriber_03::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .without_time()
            .finish();

        use tracing_subscriber_03::util::SubscriberInitExt;
        TestGuard {
            _x1: collector.set_default(),
        }
    }
}
