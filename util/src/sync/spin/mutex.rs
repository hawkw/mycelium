use crate::sync::{
    atomic::{AtomicBool, Ordering::*},
    cell::{MutPtr, UnsafeCell},
    spin::Backoff,
};
use core::{
    fmt,
    ops::{Deref, DerefMut},
};

/// A spinlock-based mutual exclusion lock for protecting shared data
///
/// This mutex will spin with an exponential backoff while waiting for the lock
/// to become available. Each mutex has a type parameter which represents
/// the data that it is protecting. The data can only be accessed through the
/// RAII guards returned from [`lock`] and [`try_lock`], which guarantees that
/// the data is only ever accessed when the mutex is locked.
///
/// # Fairness
///
/// This is *not* a fair mutex.
///
/// # Loom-specific behavior
///
/// When `cfg(loom)` is enabled, this mutex will use Loom's simulated atomics,
/// checked `UnsafeCell`, and simulated spin loop hints.
///
/// [`lock`]: Mutex::lock
/// [`try_lock`]: Mutex::try_lock
#[derive(Debug)]
pub struct Mutex<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

/// An RAII implementation of a "scoped lock" of a mutex. When this structure is
/// dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the mutex can be accessed through this guard via its
/// [`Deref`] and [`DerefMut`] implementations.
///
/// This structure is created by the [`lock`] and [`try_lock`] methods on
/// [`Mutex`].
///
/// [`lock`]: Mutex::lock
/// [`try_lock`]: Mutex::try_lock
pub struct MutexGuard<'a, T> {
    ptr: MutPtr<T>,
    locked: &'a AtomicBool,
}

impl<T> Mutex<T> {
    loom_const_fn! {
        /// Returns a new `Mutex` protecting the provided `data`.
        ///
        /// The returned `Mutex` is in an unlocked state, ready for use.
        ///
        /// # Examples
        ///
        /// ```
        /// use mycelium_util::sync::blocking::Mutex;
        ///
        /// let mutex = Mutex::new(0);
        /// ```
        #[must_use]
        pub fn new(data: T) -> Self {
            Self {
                locked: AtomicBool::new(false),
                data: UnsafeCell::new(data),
            }
        }
    }

    /// Attempts to acquire this lock without spinning
    ///
    /// If the lock could not be acquired at this time, then [`None`] is returned.
    /// Otherwise, an RAII guard is returned. The lock will be unlocked when the
    /// guard is dropped.
    ///
    /// This function will never spin.
    #[must_use]
    #[cfg_attr(test, track_caller)]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if test_dbg!(self
            .locked
            .compare_exchange(false, true, Acquire, Acquire)
            .is_ok())
        {
            Some(MutexGuard {
                ptr: self.data.get_mut(),
                locked: &self.locked,
            })
        } else {
            None
        }
    }

    /// Acquires a mutex, spinning until it is locked.
    ///
    /// This function will spin until the mutex is available to lock. Upon
    /// returning, the thread is the only thread with the lock
    /// held. An RAII guard is returned to allow scoped unlock of the lock. When
    /// the guard goes out of scope, the mutex will be unlocked.
    #[cfg_attr(test, track_caller)]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        let mut boff = Backoff::default();
        while test_dbg!(self
            .locked
            .compare_exchange(false, true, Acquire, Acquire)
            .is_err())
        {
            while test_dbg!(self.locked.load(Relaxed)) {
                boff.spin();
            }
        }

        MutexGuard {
            ptr: self.data.get_mut(),
            locked: &self.locked,
        }
    }

    /// Forcibly unlock the mutex.
    ///
    /// If a lock is currently held, it will be released, regardless of who's
    /// holding it. Of course, this is **outrageously, disgustingly unsafe** and
    /// you should never do it.
    ///
    /// # Safety
    ///
    /// This deliberately violates mutual exclusion.
    ///
    /// Only call this method when it is _guaranteed_ that no stack frame that
    /// has previously locked the mutex will ever continue executing.
    /// Essentially, this is only okay to call when the kernel is oopsing and
    /// all code running on other cores has already been killed.
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Release);
    }
}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

// === impl MutexGuard ===

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // Safety: we are holding the lock, so it is okay to dereference the
            // mut pointer.
            &*self.ptr.deref()
        }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // Safety: we are holding the lock, so it is okay to dereference the
            // mut pointer.
            self.ptr.deref()
        }
    }
}

impl<'a, T, R: ?Sized> AsRef<R> for MutexGuard<'a, T>
where
    T: AsRef<R>,
{
    #[inline]
    fn as_ref(&self) -> &R {
        self.deref().as_ref()
    }
}

impl<'a, T, R: ?Sized> AsMut<R> for MutexGuard<'a, T>
where
    T: AsMut<R>,
{
    #[inline]
    fn as_mut(&mut self) -> &mut R {
        self.deref_mut().as_mut()
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        test_dbg!(self.locked.store(false, Release));
    }
}

impl<'a, T: fmt::Debug> fmt::Debug for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<'a, T: fmt::Display> fmt::Display for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use crate::loom::{self, thread};
    use std::prelude::v1::*;
    use std::sync::Arc;

    use super::*;

    #[test]
    fn multithreaded() {
        loom::model(|| {
            let mutex = Arc::new(Mutex::new(String::new()));
            let mutex2 = mutex.clone();

            let t1 = thread::spawn(move || {
                test_info!("t1: locking...");
                let mut lock = mutex2.lock();
                test_info!("t1: locked");
                lock.push_str("bbbbb");
                test_info!("t1: dropping...");
            });

            {
                test_info!("t2: locking...");
                let mut lock = mutex.lock();
                test_info!("t2: locked");
                lock.push_str("bbbbb");
                test_info!("t2: dropping...");
            }
            t1.join().unwrap();
        });
    }

    #[test]
    fn try_lock() {
        loom::model(|| {
            let mutex = Mutex::new(42);
            // First lock succeeds
            let a = mutex.try_lock();
            assert_eq!(a.as_ref().map(|r| **r), Some(42));

            // Additional lock failes
            let b = mutex.try_lock();
            assert!(b.is_none());

            // After dropping lock, it succeeds again
            ::core::mem::drop(a);
            let c = mutex.try_lock();
            assert_eq!(c.as_ref().map(|r| **r), Some(42));
        });
    }
}
