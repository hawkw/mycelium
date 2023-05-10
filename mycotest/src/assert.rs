//! Non-panicking assertions.
//!
//! These assertions behave similarly to the assertion macros in the Rust
//! standard library, but rather than panicking, they immediately return a
//! `Result` with a [`Failed`] error.
//!
//! Since the kernel cannot currently unwind and recover from panics, this
//! allows assertions to fail without halting the kernel, allowing other tests
//! to run.

/// An error that indicates that an assertion failed.
#[derive(Clone, Default)]
pub struct Failed {
    pub expr: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub col: u32,
}

/// Returns a [`Failed`] error with the provided expression at the current
/// source code location.
#[macro_export]
macro_rules! fail {
    ($expr:expr) => {
        return Err($crate::assert::Failed {
            expr: $expr,
            file: file!(),
            line: line!(),
            col: column!(),
        });
    };
    () => {
        $crate::fail!("test failed");
    };
}

/// Asserts that a boolean expression is true.
#[macro_export]
macro_rules! assert {
    (@inner $cond:expr, $msg:expr) => {
        if !$cond {
            let cond = stringify!($cond);
            tracing::error!("assertion failed: `{cond}`{}, {}:{}:{}", $msg, file!(), line!(), column!());
            $crate::fail!(cond)
        }
    };
    ($cond:expr, $($msg:tt)+) => {
        $crate::assert!(@inner $cond, format_args!("; ", format_args!($($msg)+)))
    };
    ($cond:expr $(,)?) => {
        $crate::assert!(@inner $cond, "")
    };
}

/// Asserts that two values are equal.
#[macro_export]
macro_rules! assert_eq {
    ($left:expr, $right:expr $(,)?) => {
        $crate::assert_binop!($left, $right, ==)
    };
    ($left:expr, $right:expr, $($msg:tt)+) => {
        $crate::assert_binop!($left, $right, ==, $($msg)+)
    };
}

/// Asserts that two values are not equal.
#[macro_export]
macro_rules! assert_ne {
    ($left:expr, $right:expr $(,)?) => {
        $crate::assert_binop!($left, $right, !=)
    };
    ($left:expr, $right:expr, $($msg:tt)+) => {
        $crate::assert_binop!($left, $right, !=, $($msg)+)
    };
}

/// Asserts that the left value is greater than the right value.
#[macro_export]
macro_rules! assert_gt {
    ($left:expr, $right:expr $(,)?) => {
        $crate::assert_binop!($left, $right, >)
    };
    ($left:expr, $right:expr, $($msg:tt)*) => {
        $crate::assert_binop!($left, $right, >, $($msg)*)
    };
}

/// Asserts that the left value is less than the right value.
#[macro_export]
macro_rules! assert_lt {
    ($left:expr, $right:expr $(,)?) => {
        $crate::assert_binop!($left, $right, <)
    };
    ($left:expr, $right:expr, $($msg:tt)*) => {
        $crate::assert_binop!($left, $right, <, $($msg)*)
    };
}

/// Asserts that the left value is greater than or equal to the right value.
#[macro_export]
macro_rules! assert_gte {
    ($left:expr, $right:expr $(,)?) => {
        $crate::assert_binop!($left, $right, >=)
    };
    ($left:expr, $right:expr, $($msg:tt)*) => {
        $crate::assert_binop!($left, $right, >=, $($msg)*)
    };
}

/// Asserts that the left value is less than or equal to the right value.
#[macro_export]
macro_rules! assert_lte {
    ($left:expr, $right:expr $(,)?) => {
        $crate::assert_binop!($left, $right, <=)
    };
    ($left:expr, $right:expr, $($msg:tt)*) => {
        $crate::assert_binop!($left, $right, <=, $($msg)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! assert_binop {
    ($left:expr, $right:expr, $binop:tt) => {
        $crate::assert_binop!(@inner $left, $right, $binop, "")
    };
    ($left:expr, $right:expr, $binop:tt, $($msg:tt)+) => {
        $crate::assert_binop!(@inner $left, $right, $binop, ": {}", format_args!($($msg)+))
    };
    (@inner $left:expr, $right:expr, $binop:tt, $($msg:tt)+) => {
        let left = $left;
        let right = $right;
        if !(left $binop right) {
            let condition = concat!(stringify!($left), " ", stringify!($binop), " ", stringify!($right));
            tracing::error!(
                "assertion failed: `{condition}`\n  left: `{:?}`,\n right: `{:?}`{}, {}:{}:{}",
                left, right, format_args!($($msg)+), file!(), line!(), column!()
            );
            $crate::fail!(condition)
        }
    };
}

impl core::fmt::Debug for Failed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self {
            expr,
            file,
            line,
            col,
        } = self;
        write!(f, "assertion failed: `{expr}`, {file}:{line}:{col}",)
    }
}
