//! Waiter cells and queues to allow tasks to wait for notifications.
//!
//! This module implements two types of structure for waiting: a [`WaitCell`],
//! which stores a *single* waiting task, and a wait *queue*, which
//! stores a queue of waiting tasks.
pub(crate) mod cell;
mod queue;
pub use self::{cell::WaitCell, queue::WaitQueue};

use core::task::Poll;

/// An error indicating that a [`WaitCell`] or queue was closed while attempting
/// register a waiter.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Closed(());

pub type WaitResult = Result<(), Closed>;

pub(in crate::wait) const fn closed() -> Poll<WaitResult> {
    Poll::Ready(Err(Closed::new()))
}

pub(in crate::wait) const fn notified() -> Poll<WaitResult> {
    Poll::Ready(Ok(()))
}

impl Closed {
    pub(in crate::wait) const fn new() -> Self {
        Self(())
    }
}
