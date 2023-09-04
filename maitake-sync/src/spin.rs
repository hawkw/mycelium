//! Synchronous spinning-based synchronization primitives.
//!
//! The synchronization primitives in `maitake-sync` are _asynchronous_. They
//! are designed to be used with [`core::task`] and [`core::future`], and when
//! it is necessary to wait for another task to complete some work for the
//! current task to proceed, `maitake`'s synchronization primitives wait by
//! *yielding* to the asynchronous task scheduler to allow another task to
//! proceed.
//!
//! This module, on the other hand, provides _synchronous_ (or _blocking_)
//! synchronization primitives. Rather than yielding to the runtime, these
//! synchronization primitives will block the current CPU core (or thread, if
//! running in an environment with threads) until they are woken by other cores.
//! This is performed by *spinning*: issuing yield or pause instructions in a
//! loop until some value changes. These synchronization primitives are, in some
//! cases, necessary to implement the async synchronization primitives that form
//! `maitake-sync`'s core APIs. They are also exposed publicly so they can be
//! used in other projects, when a spinlock-based synchronization primitive is
//! needed.
//!
//! This module provides the following APIs:
//!
//! - [`Mutex`]: a synchronous [mutual exclusion] spinlock.
//! - [`InitOnce`]: a cell storing a [`MaybeUninit`](core::mem::MaybeUninit)
//!       value which must be manually initialized prior to use.
//! - [`Lazy`]: an [`InitOnce`] cell coupled with an initializer function. The
//!       [`Lazy`] cell ensures the initializer is called to initialize the
//!       value the first time it is accessed.
//!
//! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
mod mutex;
pub mod once;

pub use self::{
    mutex::*,
    once::{InitOnce, Lazy},
};
