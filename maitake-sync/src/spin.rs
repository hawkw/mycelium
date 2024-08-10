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
//! - [`Spinlock`]: a synchronous [mutual exclusion] spinlock, which implements
//!       the [`blocking::RawMutex`] and [`blocking::ScopedRawMutex`] traits.
//! - [`RwSpinlock`]: a synchronous [reader-writer] spinlock, which implements
//!       the [`blocking::RawRwLock`] trait.
//! - [`InitOnce`]: a cell storing a [`MaybeUninit`](core::mem::MaybeUninit)
//!       value which must be manually initialized prior to use.
//! - [`Lazy`]: an [`InitOnce`] cell coupled with an initializer function. The
//!       [`Lazy`] cell ensures the initializer is called to initialize the
//!       value the first time it is accessed.
//!
//! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
//! [reader-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
pub mod once;

pub use self::once::{InitOnce, Lazy};

use crate::{
    blocking,
    loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*},
    util::{fmt, Backoff},
};
// This import is pulled out because we want to reference it in docs, and
// importing `use crate::blocking; use blocking::{RawMutex, RawRwLock};` makes
// `blocking` appear used even if it's not referenced directly, while
// `use crate::blocking::{self, RawMutex, RawRwLock};` causes the `self` to
// appear "unused" to rustc. Yes, this is stupid, but this workaround felt
// better than `allow(unused_imports)`.
use blocking::{RawMutex, RawRwLock};

/// A spinlock-based [`RawMutex`] implementation.
///
/// This mutex will spin with an exponential backoff while waiting for the lock
/// to become available.
///
/// This type implements the [`RawMutex`] and
/// [`ScopedRawMutex`](mutex_traits::ScopedRawMutex) traits from the
/// [`mutex-traits`] crate. This allows it to be used with the
/// [`blocking::Mutex`] type when a spinlock-based mutex is needed.
#[derive(Debug)]
pub struct Spinlock {
    locked: AtomicBool,
}

/// A spinlock-based [`RawRwLock`] implementation.
///
/// This type implements the [`blocking::RawRwLock`] trait. This allows it to be
/// used with the [`blocking::RwLock`] type when a spinlock-based reader-writer
/// lock is needed.
pub struct RwSpinlock {
    state: AtomicUsize,
}

// === impl RawSpinlock ===

impl Spinlock {
    loom_const_fn! {
        /// Returns a new `Spinlock`, in the unlocked state.
        pub fn new() -> Self {
            Self { locked: AtomicBool::new(false) }
        }
    }

    #[inline]
    #[must_use]
    fn is_locked(&self) -> bool {
        self.locked.load(Relaxed)
    }
}

impl Default for Spinlock {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl RawMutex for Spinlock {
    type GuardMarker = ();

    #[cfg_attr(test, track_caller)]
    fn lock(&self) {
        let mut boff = Backoff::default();
        while test_dbg!(self
            .locked
            .compare_exchange(false, true, Acquire, Acquire)
            .is_err())
        {
            while test_dbg!(self.is_locked()) {
                boff.spin();
            }
        }
    }

    #[cfg_attr(test, track_caller)]
    #[inline]
    fn try_lock(&self) -> bool {
        test_dbg!(self
            .locked
            .compare_exchange(false, true, Acquire, Acquire)
            .is_ok())
    }

    #[cfg_attr(test, track_caller)]
    #[inline]
    unsafe fn unlock(&self) {
        test_dbg!(self.locked.store(false, Release));
    }

    #[inline]
    fn is_locked(&self) -> bool {
        Spinlock::is_locked(self)
    }
}

#[cfg(not(loom))]
impl blocking::ConstInit for Spinlock {
    // As usual, clippy is totally wrong about this --- the whole point of this
    // constant is to create a *new* spinlock every time.
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Spinlock::new();
}

const UNLOCKED: usize = 0;
const WRITER: usize = 1 << 0;
const READER: usize = 1 << 1;

impl RwSpinlock {
    loom_const_fn! {
        pub(crate) fn new() -> Self {
            Self {
                state: AtomicUsize::new(UNLOCKED),
            }
        }
    }

    pub(crate) fn reader_count(&self) -> usize {
        self.state.load(Relaxed) >> 1
    }
}

unsafe impl RawRwLock for RwSpinlock {
    type GuardMarker = ();

    #[cfg_attr(test, track_caller)]
    fn lock_shared(&self) {
        let mut boff = Backoff::new();
        while !self.try_lock_shared() {
            boff.spin();
        }
    }

    #[cfg_attr(test, track_caller)]
    fn try_lock_shared(&self) -> bool {
        // Add a reader.
        let state = test_dbg!(self.state.fetch_add(READER, Acquire));

        // Ensure we don't overflow the reader count and clobber the lock's
        // state.
        assert!(
            state < usize::MAX - (READER * 2),
            "read lock counter overflow! this is very bad"
        );

        // Is the write lock held? If so, undo the increment and bail.
        if state & WRITER == 1 {
            test_dbg!(self.state.fetch_sub(READER, Release));
            false
        } else {
            true
        }
    }

    #[cfg_attr(test, track_caller)]
    #[inline]
    unsafe fn unlock_shared(&self) {
        let _val = test_dbg!(self.state.fetch_sub(READER, Release));
        debug_assert_eq!(
            _val & WRITER,
            0,
            "tried to drop a read guard while write locked, something is Very Wrong!"
        )
    }

    #[cfg_attr(test, track_caller)]
    fn lock_exclusive(&self) {
        let mut backoff = Backoff::new();

        // Wait for the lock to become available and set the `WRITER` bit.
        //
        // Note that, unlike the `lock_shared` method, we don't use the
        // `try_lock_exclusive` method here, as we would like to use
        // `compare_exchange_weak` to allow spurious failures for improved
        // performance. The `try_lock_exclusive` method  cannot use
        // `compare_exchange_weak`, because it will never retry, and
        // a spurious failure means we would incorrectly fail to lock the RwLock
        // when we should have successfully locked it.
        while test_dbg!(self
            .state
            .compare_exchange_weak(UNLOCKED, WRITER, Acquire, Relaxed))
        .is_err()
        {
            test_dbg!(backoff.spin());
        }
    }

    #[cfg_attr(test, track_caller)]
    #[inline]
    fn try_lock_exclusive(&self) -> bool {
        test_dbg!(self
            .state
            .compare_exchange(UNLOCKED, WRITER, Acquire, Relaxed))
        .is_ok()
    }

    #[cfg_attr(test, track_caller)]
    #[inline]
    unsafe fn unlock_exclusive(&self) {
        let _val = test_dbg!(self.state.swap(UNLOCKED, Release));
    }

    #[inline]
    fn is_locked(&self) -> bool {
        self.state.load(Relaxed) & (WRITER | READER) != 0
    }

    #[inline]
    fn is_locked_exclusive(&self) -> bool {
        self.state.load(Relaxed) & WRITER == 1
    }
}

#[cfg(not(loom))]
impl blocking::ConstInit for RwSpinlock {
    // As usual, clippy is totally wrong about this --- the whole point of this
    // constant is to create a *new* spinlock every time.
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = RwSpinlock::new();
}

impl fmt::Debug for RwSpinlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = &self.state.load(Relaxed);
        f.debug_struct("RwSpinlock")
            // N.B.: this does *not* use the `reader_count` and `has_writer`
            // methods *intentionally*, because those two methods perform
            // independent reads of the lock's state, and may race with other
            // lock operations that occur concurrently. If, for example, we read
            // a non-zero reader count, and then read the state again to check
            // for a writer, the reader may have been released and a write lock
            // acquired between the two reads, resulting in the `Debug` impl
            // displaying an invalid state when the lock was not actually *in*
            // such a state!
            //
            // Therefore, we instead perform a single load to snapshot the state
            // and unpack both the reader count and the writer count from the
            // lock.
            .field("readers", &(state >> 1))
            .field("writer", &(state & WRITER))
            .finish()
    }
}
