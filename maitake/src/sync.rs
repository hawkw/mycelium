//! Synchronization primitives
//!
//! This module contains the following asynchronous synchronization primitives:
//!
//! - [`Mutex`]: a fairly queued, asynchronous mutual exclusion lock.
pub mod mutex;
#[doc(inline)]
pub use self::mutex::{Mutex, MutexGuard, OwnedMutexGuard};

#[cfg(test)]
mod tests;
