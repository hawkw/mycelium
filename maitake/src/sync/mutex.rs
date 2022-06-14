use crate::{
    loom::cell::{MutPtr, UnsafeCell},
    wait::queue::{self, WaitQueue},
};
use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};
use mycelium_util::{fmt, unreachable_unchecked};
use pin_project::pin_project;

pub struct Mutex<T> {
    wait: WaitQueue,
    data: UnsafeCell<T>,
}

pub struct MutexGuard<'a, T> {
    /// /!\ WARNING: semi-load-bearing drop order /!\
    ///
    /// This struct's field ordering is important.
    data: MutPtr<T>,
    _wake: WakeOnDrop<'a, T>,
}

#[pin_project]
pub struct Lock<'a, T> {
    #[pin]
    wait: queue::Wait<'a>,
    mutex: &'a Mutex<T>,
}

/// This is used in order to ensure that the wakeup is performed only *after*
/// the data ptr is dropped, in order to keep `loom` happy.
struct WakeOnDrop<'a, T>(&'a Mutex<T>);

// === impl Mutex ===

impl<T> Mutex<T> {
    loom_const_fn! {
        #[must_use]
        pub fn new(data: T) -> Self {
            Self {
                // The queue must start with a single stored wakeup, so that the
                // first task that tries to acquire the lock will succeed
                // immediately.
                wait: WaitQueue::new_woken(),
                data: UnsafeCell::new(data),
            }
        }
    }

    pub fn lock(&self) -> Lock<'_, T> {
        Lock {
            wait: self.wait.wait(),
            mutex: self,
        }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.wait.try_wait() {
            Poll::Pending => None,
            Poll::Ready(Ok(_)) => Some(unsafe {
                // safety: we have just acquired the lock
                self.guard()
            }),
            Poll::Ready(Err(_)) => unsafe {
                unreachable_unchecked!("`Mutex` never calls `WaitQueue::close`")
            },
        }
    }

    /// Constructs a new `MutexGuard` for this `Mutex`.
    ///
    /// # Safety
    ///
    /// This may only be called once a lock has been acquired.
    unsafe fn guard(&self) -> MutexGuard<'_, T> {
        MutexGuard {
            _wake: WakeOnDrop(self),
            data: self.data.get_mut(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mutex")
            .field("data", &fmt::opt(&self.try_lock()).or_else("<locked>"))
            .field("wait", &self.wait)
            .finish()
    }
}

unsafe impl<T> Send for Mutex<T> where T: Send {}
unsafe impl<T> Sync for Mutex<T> where T: Send {}

// === impl Lock ===

impl<'a, T> Future for Lock<'a, T> {
    type Output = MutexGuard<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.wait.poll(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(_)) => unsafe {
                unreachable_unchecked!("`Mutex` never calls `WaitQueue::close`")
            },
            Poll::Pending => return Poll::Pending,
        }

        let guard = unsafe {
            // safety: we have just acquired the lock.
            this.mutex.guard()
        };
        Poll::Ready(guard)
    }
}

// === impl MutexGuard ===

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // safety: we are holding the lock
            &*self.data.deref()
        }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // safety: we are holding the lock
            self.data.deref()
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

unsafe impl<T> Send for MutexGuard<'_, T> where T: Send {}
unsafe impl<T> Sync for MutexGuard<'_, T> where T: Send + Sync {}

impl<'a, T> Drop for WakeOnDrop<'a, T> {
    fn drop(&mut self) {
        self.0.wait.wake()
    }
}
