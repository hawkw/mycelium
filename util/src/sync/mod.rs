//! Synchronization primitives.

#[cfg(any(test, feature = "loom"))]
pub use loom::sync::atomic;

#[cfg(all(not(test), not(feature = "loom")))]
pub use core::sync::atomic;

mod mutex;
pub use self::mutex::*;
