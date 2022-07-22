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
#[macro_export]
macro_rules! unreachable_unchecked {
    () => ({
        #[cfg(any(test, debug_assertions))]
        panic!(
            "internal error: entered unreachable code \n\",
            /!\\ EXTREMELY SERIOUS WARNING: in release mode, this would have been \n\
            \x32   `unreachable_unchecked`! This could result in undefine behavior. \n\
            \x32   Please double- or triple-check any assumptions about code which \n\
            \x32   could lead to this being triggered."
        );
        #[allow(unreachable_code)] // lol
        {
            core::hint::unreachable_unchecked();
        }
    });
    ($msg:expr) => ({
        $crate::unreachable_unchecked!("{}", $msg)
    });
    ($msg:expr,) => ({
        $crate::unreachable_unchecked!($msg)
    });
    ($fmt:expr, $($arg:tt)*) => ({
        #[cfg(any(test, debug_assertions))]
        panic!(
            concat!(
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

/// Variadic version of [`core::cmp::max`].
#[macro_export]
macro_rules! max {
    ($arg:expr) => { $arg };
    ($arg1:expr, $($arg:expr),+) => {
        core::cmp::max($arg1, $crate::max!( $($arg),+ ))
    };
}

/// Variadic version of [`core::cmp::min`].
#[macro_export]
macro_rules! min {
    ($arg:expr) => { $arg };
    ($arg1:expr, $($arg:expr),+) => {
        core::cmp::min($arg1, $crate::min!( $($arg),+ ))
    };
}
