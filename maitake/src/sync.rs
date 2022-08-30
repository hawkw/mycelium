//! Synchronization primitives
//!
//! This module contains the following asynchronous synchronization primitives:
//!
//! - [`Mutex`]: a fairly queued, asynchronous mutual exclusion lock.
//! - [`Semaphore`]: an asynchronous [counting semaphore], for limiting the
//!   number of tasks which may run concurrently
//! - [`WaitCell`], which stores a *single* waiting task
//! - [`WaitQueue`], a queue of waiting tasks, which are woken in first-in,
//!   first-out order
//! - [`WaitMap`], a set of waiting tasks associated with keys, in which a task
//!   can be woken by its key
//!
pub mod mutex;
pub mod semaphore;
pub(crate) mod wait_cell;
pub mod wait_map;
pub mod wait_queue;

#[cfg(feature = "alloc")]
#[doc(inline)]
pub use self::mutex::OwnedMutexGuard;
#[doc(inline)]
pub use self::mutex::{Mutex, MutexGuard};
#[doc(inline)]
pub use self::semaphore::Semaphore;
pub use self::wait_cell::WaitCell;
#[doc(inline)]
pub use self::wait_map::WaitMap;
#[doc(inline)]
pub use self::wait_queue::WaitQueue;

use core::task::Poll;

/// An error indicating that a [`WaitCell`] or [`WaitQueue`] was closed while
/// attempting register a waiter.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Closed(());

pub type WaitResult<T> = Result<T, Closed>;

pub(in crate::sync) const fn closed<T>() -> Poll<WaitResult<T>> {
    Poll::Ready(Err(Closed::new()))
}

impl Closed {
    pub(in crate::sync) const fn new() -> Self {
        Self(())
    }
}
