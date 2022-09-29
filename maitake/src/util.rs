use core::ptr::NonNull;

mod wake_batch;
pub(crate) use self::wake_batch::WakeBatch;

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

macro_rules! if_atomic_u64 {
    ($($item:item)*) => {
        $(
            // NOTE: `target_arch` values of "arm", "mips", and
            // "powerpc" refer specifically to the 32-bit versions
            // of those architectures; the 64-bit architectures get
            // the `target_arch` strings "aarch64", "mips64", and
            // "powerpc64", respectively.
            #[cfg(not(any(
                target_arch = "arm",
                target_arch = "mips",
                target_arch = "powerpc",
                target_arch = "riscv32",
            )))]
            $item
        )*
    }
}

macro_rules! if_no_atomic_u64 {
    ($($item:item)*) => {
        $(
            // NOTE: `target_arch` values of "arm", "mips", and
            // "powerpc" refer specifically to the 32-bit versions
            // of those architectures; the 64-bit architectures get
            // the `target_arch` strings "aarch64", "mips64", and
            // "powerpc64", respectively.
            #[cfg(any(
                target_arch = "arm",
                target_arch = "mips",
                target_arch = "powerpc",
                target_arch = "riscv32",
            ))]
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
pub(crate) unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new_unchecked(ptr)
}

#[cfg(all(test, not(loom)))]
pub(crate) use self::test::trace_init;

pub(crate) fn expect_display<T, E: core::fmt::Display>(result: Result<T, E>, msg: &str) -> T {
    match result {
        Ok(t) => t,
        Err(error) => panic!("{msg}: {error}"),
    }
}

#[cfg(test)]
pub(crate) mod test {
    #[cfg(not(loom))]
    pub(crate) fn trace_init() {
        trace_init_with_default("maitake=debug,cordyceps=debug");
    }

    #[cfg(not(loom))]
    pub(crate) fn trace_init_with_default(default: &str) {
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
        let _ = tracing_02::collect::set_global_default(collector);
    }

    #[allow(dead_code)]
    pub(crate) fn assert_send<T: Send>() {}

    #[allow(dead_code)]
    pub(crate) fn assert_sync<T: Sync>() {}
    pub(crate) fn assert_send_sync<T: Send + Sync>() {}
}
