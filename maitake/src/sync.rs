//! Synchronization primitives
//!
//! This module contains the following asynchronous synchronization primitives:
//!
//! - [`Mutex`]: a fairly queued, asynchronous mutual exclusion lock.
pub mod mutex;
#[cfg(feature = "alloc")]
#[doc(inline)]
pub use self::mutex::OwnedMutexGuard;
#[doc(inline)]
pub use self::mutex::{Mutex, MutexGuard};

#[cfg(test)]
mod tests;
