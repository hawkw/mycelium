//! Waiter cells and queues to allow tasks to wait for notifications.
//!
//! This module implements three types of structure for waiting:
//!
//! - [`WaitCell`], which stores a *single* waiting task
//! - [`WaitQueue`], a queue of waiting tasks, which are woken in first-in,
//!   first-out order
//! - [`WaitMap`], a set of waiting tasks associated with keys, in which a task
//!   can be woken by its key
pub(crate) mod cell;
pub mod map;
pub mod queue;

pub use self::cell::WaitCell;
#[doc(inline)]
pub use self::map::WaitMap;
#[doc(inline)]
pub use self::queue::WaitQueue;

use core::task::Poll;

/// An error indicating that a [`WaitCell`] or [`WaitQueue`] was closed while
/// attempting register a waiter.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Closed(());

pub type WaitResult<T> = Result<T, Closed>;

pub(in crate::wait) const fn closed<T>() -> Poll<WaitResult<T>> {
    Poll::Ready(Err(Closed::new()))
}

impl Closed {
    pub(in crate::wait) const fn new() -> Self {
        Self(())
    }
}
