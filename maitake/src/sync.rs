//! Asynchronous synchronization primitives
//!
//! This module contains the following synchronization primitives:
//!
//! - [`Mutex`]: a fairly queued, asynchronous [mutual exclusion lock], for
//!       protecting shared data
//! - [`Semaphore`]: an asynchronous [counting semaphore], for limiting the
//!       number of tasks which may run concurrently
//! - [`WaitCell`], a cell that stores a *single* waiting task's [`Waker`], so
//!       that the task can be woken by another task,
//! - [`WaitQueue`], a queue of waiting tasks, which are woken in first-in,
//!       first-out order
//! - [`WaitMap`], a set of waiting tasks associated with keys, in which a task
//!       can be woken by its key
//!
//! [mutual exclusion lock]:https://en.wikipedia.org/wiki/Mutual_exclusion
//! [counting semaphore]: https://en.wikipedia.org/wiki/Semaphore_(programming)
//! [`Waker`]: core::task::Waker
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

/// An error indicating that a [`WaitCell`], [`WaitQueue`], [`WaitMap`], or
/// [`Semaphore`] was closed while attempting to register a waiter.
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
