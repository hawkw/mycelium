use super::atomic::{AtomicBool, Ordering::*};
use crate::cell::{MutPtr, UnsafeCell};
use core::{
    fmt,
    ops::{Deref, DerefMut},
};

/// A simple spinlock ensuring mutual exclusion.
#[derive(Debug)]
pub struct Mutex<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

pub struct MutexGuard<'a, T> {
    ptr: MutPtr<T>,
    locked: &'a AtomicBool,
}

impl<T> Mutex<T> {
    #[cfg(not(loom))]
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    #[cfg(loom)]
    pub fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self
            .locked
            .compare_exchange(false, true, Acquire, Acquire)
            .is_ok()
        {
            Some(MutexGuard {
                ptr: self.data.get_mut(),
                locked: &self.locked,
            })
        } else {
            None
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        let mut boff = super::Backoff::default();
        while self
            .locked
            .compare_exchange(false, true, Acquire, Acquire)
            .is_err()
        {
            while self.locked.load(Relaxed) {
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
        self.locked.store(false, Release);
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

#[cfg(all(test, loom))]
mod tests {
    use loom::thread;
    use std::prelude::v1::*;
    use std::sync::Arc;

    use super::*;

    #[test]
    fn multithreaded() {
        loom::model(|| {
            let mutex = Arc::new(Mutex::new(String::new()));
            let mutex2 = mutex.clone();

            let t1 = thread::spawn(move || {
                println!("t1: locking...");
                let mut lock = mutex2.lock();
                println!("t1: locked");
                lock.push_str("bbbbb");
                println!("t1: dropping...");
            });

            {
                println!("t2: locking...");
                let mut lock = mutex.lock();
                println!("t2: locked");
                lock.push_str("bbbbb");
                println!("t2: dropping...");
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
