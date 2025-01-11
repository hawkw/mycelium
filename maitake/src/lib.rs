#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(docsrs, loom)))]
#![cfg_attr(not(test), no_std)]
#![allow(unused_unsafe)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[macro_use]
pub(crate) mod util;
#[macro_use]
pub(crate) mod trace;
pub(crate) mod loom;

pub mod future;
pub mod scheduler;
pub mod task;
pub mod time;

pub mod sync {
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
    //! **Note**: `maitake`'s synchronization primitives *do not* require the
    //! `maitake` runtime, and can be used with any async executor. Therefore,
    //! they are provided by a separate [`maitake-sync`] crate, which can be
    //! used without depending on the rest of the `maitake` runtime. This module
    //! re-exports these APIs from [`maitake-sync`].
    //!
    //! [primitives]: https://wiki.osdev.org/Synchronization_Primitives
    //! [tasks]: crate::task
    //! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
    //! [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
    //! [counting semaphore]: https://en.wikipedia.org/wiki/Semaphore_(programming)
    //! [`Waker`]: core::task::Waker
    //! [`maitake-sync`]: https://crates.io/crates/maitake-sync
    pub use maitake_sync::*;
}
