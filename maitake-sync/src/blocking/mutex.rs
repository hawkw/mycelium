use crate::{
    loom::cell::{MutPtr, UnsafeCell},
    spin::Spinlock,
    util::fmt,
};
use core::ops::{Deref, DerefMut};

pub use mutex_traits::{RawMutex, ScopedRawMutex};

/// A blocking mutual exclusion lock for protecting shared data.
/// Each mutex has a type parameter which represents
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
pub struct Mutex<T, Lock = Spinlock> {
    lock: Lock,
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
#[must_use = "if unused, the `Mutex` will immediately unlock"]
pub struct MutexGuard<'a, T, Lock: RawMutex = Spinlock> {
    ptr: MutPtr<T>,
    lock: &'a Lock,
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
        /// use maitake_sync::spin::Mutex;
        ///
        /// let mutex = Mutex::new(0);
        /// ```
        #[must_use]
        pub fn new(data: T) -> Self {
            Self {
                lock: Spinlock::new(),
                data: UnsafeCell::new(data),
            }
        }
    }
}

impl<T, Lock: mutex_traits::ConstInit> Mutex<T, Lock> {
    loom_const_fn! {
        /// Returns a new `Mutex` protecting the provided `data`, using the
        /// `Lock` type parameter as the raw mutex implementation.
        ///
        /// This constructor is used to override the internal implementation of
        /// mutex operations, with an implementation of the [`lock_api::RawMutex`]
        /// trait. By default, the [`Mutex::new`] constructor uses a [`Spinlock`] as
        /// the underlying raw mutex implementation, which will spin until the mutex
        /// is unlocked, without using platform-specific or OS-specific blocking
        /// mechanisms.
        ///
        /// The returned `Mutex` is in an unlocked state, ready for use.
        #[must_use]
        pub fn with_raw_mutex(data: T) -> Self {
            Self {
                lock: Lock::INIT,
                data: UnsafeCell::new(data),
            }
        }
    }
}

impl<T, Lock: ScopedRawMutex> Mutex<T, Lock> {
    /// Lock this `Mutex`, blocking if it is not currently unlocked, and call
    /// `f()` with the locked data once the lock is acquired.
    ///
    /// When the `Mutex` is unlocked, this method locks it, calls `f()` with the
    /// data protected by the `Mutex`, and then unlocks the `Mutex` and returns
    /// the result of `f()`. If the `Mutex` is locked, this method blocks until
    /// it is unlocked, and then takes the lock.
    ///
    /// To return immediately rather than blocking, use [`Mutex::try_with_lock`]
    /// instead.
    ///
    /// This method is available when the `Mutex`'s `Lock` type parameter
    /// implements the [`ScopedRawMutex`] trait.
    #[track_caller]
    pub fn with_lock<U>(&self, f: impl FnOnce(&mut T) -> U) -> U {
        self.lock.with_lock(|| {
            self.data.with_mut(|data| unsafe {
                // Safety: we just locked the mutex.
                f(&mut *data)
            })
        })
    }

    /// Attempt to lock this `Mutex` without blocking and call `f()` with the
    /// locked data if the lock is acquired.
    ///
    /// If the `Mutex` is unlocked, this method locks it, calls `f()` with the
    /// data protected by the `Mutex`, and then unlocks the `Mutex` and returns
    /// [`Some`]`(U)`. Otherwise, if the lock is already held, this method
    /// returns `None` immediately, without blocking.
    ///
    /// To block until the `Mutex` is unlocked instead of returning `None`, use
    /// [`Mutex::with_lock`] instead.
    ///
    /// This method is available when the `Mutex`'s `Lock` type parameter
    /// implements the [`ScopedRawMutex`] trait.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(U)` if the lock was acquired, containing the result of
    ///   `f()`.
    /// - [`None`] if the lock is currently held and could not be acquired
    ///   without blocking.
    #[track_caller]
    pub fn try_with_lock<U>(&self, f: impl FnOnce(&mut T) -> U) -> Option<U> {
        self.lock.try_with_lock(|| {
            self.data.with_mut(|data| unsafe {
                // Safety: we just locked the mutex.
                f(&mut *data)
            })
        })
    }
}

impl<T, Lock> Mutex<T, Lock>
where
    Lock: RawMutex,
{
    fn guard(&self) -> MutexGuard<'_, T, Lock> {
        MutexGuard {
            ptr: self.data.get_mut(),
            lock: &self.lock,
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
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T, Lock>> {
        if self.lock.try_lock() {
            Some(self.guard())
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
    pub fn lock(&self) -> MutexGuard<'_, T, Lock> {
        self.lock.lock();
        self.guard()
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
        self.lock.unlock()
    }

    /// Consumes this `Mutex`, returning the guarded data.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the `Mutex` mutably, no actual locking needs to
    /// take place -- the mutable borrow statically guarantees no locks exist.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut lock = maitake_sync::spin::Mutex::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.lock(), 10);
    /// ```
    pub fn get_mut(&mut self) -> &mut T {
        unsafe {
            // Safety: since this call borrows the `Mutex` mutably, no actual
            // locking needs to take place -- the mutable borrow statically
            // guarantees no locks exist.
            self.data.with_mut(|data| &mut *data)
        }
    }
}

impl<T: Default, Lock: Default> Default for Mutex<T, Lock> {
    fn default() -> Self {
        Self {
            lock: Default::default(),
            data: UnsafeCell::new(Default::default()),
        }
    }
}

impl<T, Lock> fmt::Debug for Mutex<T, Lock>
where
    T: fmt::Debug,
    Lock: ScopedRawMutex,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.try_with_lock(|data| {
            f.debug_struct("Mutex")
                .field("data", data)
                .field("lock", &format_args!("{}", core::any::type_name::<Lock>()))
                .finish()
        })
        .unwrap_or_else(|| {
            f.debug_struct("Mutex")
                .field("data", &format_args!("<locked>"))
                .field("lock", &format_args!("{}", core::any::type_name::<Lock>()))
                .finish()
        })
    }
}

unsafe impl<T: Send, Lock> Send for Mutex<T, Lock> {}
unsafe impl<T: Send, Lock> Sync for Mutex<T, Lock> {}

// === impl MutexGuard ===

impl<T, Lock: RawMutex> Deref for MutexGuard<'_, T, Lock> {
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

impl<T, Lock: RawMutex> DerefMut for MutexGuard<'_, T, Lock> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // Safety: we are holding the lock, so it is okay to dereference the
            // mut pointer.
            self.ptr.deref()
        }
    }
}

impl<T, Lock, R: ?Sized> AsRef<R> for MutexGuard<'_, T, Lock>
where
    T: AsRef<R>,
    Lock: RawMutex,
{
    #[inline]
    fn as_ref(&self) -> &R {
        self.deref().as_ref()
    }
}

impl<T, Lock, R: ?Sized> AsMut<R> for MutexGuard<'_, T, Lock>
where
    T: AsMut<R>,
    Lock: RawMutex,
{
    #[inline]
    fn as_mut(&mut self) -> &mut R {
        self.deref_mut().as_mut()
    }
}

impl<T, Lock> Drop for MutexGuard<'_, T, Lock>
where
    Lock: RawMutex,
{
    #[inline]
    #[cfg_attr(test, track_caller)]
    fn drop(&mut self) {
        unsafe { self.lock.unlock() }
    }
}

impl<T, Lock> fmt::Debug for MutexGuard<'_, T, Lock>
where
    T: fmt::Debug,
    Lock: RawMutex,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T, Lock> fmt::Display for MutexGuard<'_, T, Lock>
where
    T: fmt::Display,
    Lock: RawMutex,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

unsafe impl<T, Lock> Send for MutexGuard<'_, T, Lock>
where
    T: Send,
    Lock: RawMutex,
    Lock::GuardMarker: Send,
{
}
unsafe impl<T, Lock> Sync for MutexGuard<'_, T, Lock>
where
    T: Send,
    Lock: RawMutex,
    Lock::GuardMarker: Send,
{
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
                tracing::info!("t1: locking...");
                let mut lock = mutex2.lock();
                tracing::info!("t1: locked");
                lock.push_str("bbbbb");
                tracing::info!("t1: dropping...");
            });

            {
                tracing::info!("t2: locking...");
                let mut lock = mutex.lock();
                tracing::info!("t2: locked");
                lock.push_str("bbbbb");
                tracing::info!("t2: dropping...");
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
