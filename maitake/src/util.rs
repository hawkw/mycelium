use core::ptr::NonNull;

#[cfg(not(all(test, loom)))]
pub(crate) use tracing_02 as tracing;

#[cfg(all(test, loom))]
pub(crate) use tracing_01 as tracing;

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
                crate::util::tracing::debug!("{} = {:?}", stringify!($e), &e);
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
        crate::util::tracing::debug!($($args)+);
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
