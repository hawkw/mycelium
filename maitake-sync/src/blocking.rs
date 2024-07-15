//! Synchronous (blocking) synchronization primitives.
//!
//! The core synchronization primitives in `maitake-sync`, such as
//! [`Mutex`](crate::Mutex),  [`RwLock`](crate::RwLock), and
//! [`WaitQueue`](crate::WaitQueue) are _asynchronous_. They are designed to be
//! used with [`core::task`] and [`core::future`], and when it is necessary to
//! wait for another task to complete some work for the current task to proceed,
//! `maitake`'s synchronization primitives wait by *yielding* to the
//! asynchronous task scheduler to allow another task to proceed.
//!
//! This module, on the other hand, provides _synchronous_ (or _blocking_)
//! synchronization primitives. Rather than yielding to the runtime, these
//! synchronization primitives will block the current CPU core (or thread, if
//! running in an environment with threads) until they are woken by other cores.
//! These synchronization primitives are, in some cases, necessary to implement
//! the async synchronization primitives that form `maitake-sync`'s core APIs.
//! They are also exposed publicly in this module so that they can be used in
//! other projects when a blocking-based synchronization primitive is needed.\
//!
//! This module provides the following synchronization primitive types:
//!
//! - [`Mutex`]: a synchronous [mutual exclusion] lock.
//! - [`RwLock`]: a synchronous [reader-writer] lock.
//!
//! # `lock_api` support
//!
//! By default, the [`Mutex`] and [`RwLock`] types are implemented using simple
//! _[spinlocks_]_, which wait for the lock to become available by _spinning_:
//! repeatedly checking an atomic value in a loop, executing [spin-loop hint
//! instructions] until the lock value changes. These spinlock implementations
//! are represented by the [`Spinlock`] and [`RwSpinlock`] types in the
//! [`spin`] module. Spinlocks are a
//!
//!
//! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
//! [reader-writer lock]:
//!     https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
//! [spinlocks]: https://en.wikipedia.org/wiki/Spinlock
//! [spin-loop hint instructions]: core::hint::spin_loop
//! [`Spinlock`]: crate::spin::Spinlock
//! [`RwSpinlock`]: crate::spin::RwSpinlock
mod mutex;
mod rwlock;

pub use self::{mutex::*, rwlock::*};
