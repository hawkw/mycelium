//! Asynchronous synchronization primitives
//!
//! This module contains the following synchronization primitives:
//!
//! - [`Mutex`]: a fairly queued, asynchronous [mutual exclusion lock], for
//!       protecting shared data
//! - [`RwLock`]: a fairly queued, asynchronous [readers-writer lock], which
//!       allows concurrent read access to shared data while ensuring write
//!       access is exclusive
//! - [`Semaphore`]: an asynchronous [counting semaphore], for limiting the
//!       number of tasks which may run concurrently
//! - [`WaitCell`], a cell that stores a *single* waiting task's [`Waker`], so
//!       that the task can be woken by another task,
//! - [`WaitQueue`], a queue of waiting tasks, which are woken in first-in,
//!       first-out order
//! - [`WaitMap`], a set of waiting tasks associated with keys, in which a task
//!       can be woken by its key
//!
//! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
//! [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
//! [counting semaphore]: https://en.wikipedia.org/wiki/Semaphore_(programming)
//! [`Waker`]: core::task::Waker
#![warn(missing_docs, missing_debug_implementations)]
pub mod mutex;
pub mod rwlock;
pub mod semaphore;
pub mod wait_cell;
pub mod wait_map;
pub mod wait_queue;

#[cfg(feature = "alloc")]
#[doc(inline)]
pub use self::mutex::OwnedMutexGuard;
#[doc(inline)]
pub use self::mutex::{Mutex, MutexGuard};
#[doc(inline)]
pub use self::rwlock::{RwLock, RwLockReadGuard, RwLockWriteGuard};
#[doc(inline)]
pub use self::semaphore::Semaphore;
#[doc(inline)]
pub use self::wait_cell::WaitCell;
#[doc(inline)]
pub use self::wait_map::WaitMap;
#[doc(inline)]
pub use self::wait_queue::WaitQueue;

use core::task::Poll;

/// An error indicating that a [`WaitCell`], [`WaitQueue`] or [`Semaphore`] was
/// closed while attempting to register a waiting task.
///
/// This error is returned by the [`WaitCell::wait`], [`WaitQueue::wait`] and
/// [`Semaphore::acquire`] methods.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Closed(());

/// The result of waiting on a [`WaitQueue`] or [`Semaphore`].
pub type WaitResult<T> = Result<T, Closed>;

pub(in crate::sync) const fn closed<T>() -> Poll<WaitResult<T>> {
    Poll::Ready(Err(Closed::new()))
}

impl Closed {
    pub(in crate::sync) const fn new() -> Self {
        Self(())
    }
}
