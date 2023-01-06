//! Asynchronous synchronization primitives
//!
//! [_Synchronization primitives_][primitives] are tools for implementing
//! synchronization between [tasks]: to control which tasks can run at any given
//! time, and in what order, and to coordinate tasks' access to shared
//! resources. Typically, this synchronization involves some form of _waiting_.
//! In asynchronous systems, synchronization primitives allow tasks to wait by
//! yielding to the runtime scheduler, so that other tasks may run while they
//! are waiting. This module provides asynchronous implementations of common
//! synchronization primitives.
//!
//! The following synchronization primitives are provided:
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
//! [primitives]: https://wiki.osdev.org/Synchronization_Primitives
//! [tasks]: crate::task
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
#[cfg(feature = "alloc")]
#[doc(inline)]
pub use self::rwlock::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard};
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

impl core::fmt::Display for Closed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad("closed")
    }
}

feature! {
    #![mycelium_core_error]
    impl core::error::Error for Closed {}
}
