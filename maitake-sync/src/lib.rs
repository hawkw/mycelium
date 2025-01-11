#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(docsrs, loom)))]
#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![warn(missing_docs, missing_debug_implementations)]

#[cfg(any(feature = "alloc", test))]
extern crate alloc;

pub(crate) mod loom;

#[macro_use]
pub mod util;

pub mod blocking;
pub mod mutex;
pub mod rwlock;
pub mod semaphore;
pub mod spin;
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

pub(crate) const fn closed<T>() -> Poll<WaitResult<T>> {
    Poll::Ready(Err(Closed::new()))
}

impl Closed {
    pub(crate) const fn new() -> Self {
        Self(())
    }
}

impl core::fmt::Display for Closed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad("closed")
    }
}

feature! {
    #![feature = "core-error"]
    impl core::error::Error for Closed {}
}
