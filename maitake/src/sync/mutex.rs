use crate::{
    loom::cell::{MutPtr, UnsafeCell},
    wait::queue::{self, WaitQueue},
};
use core::{
    future::Future,
    ops,
    pin::Pin,
    task::{Context, Poll},
};
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
}

// === impl Lock ===

impl<'a, T> Future for Lock<'a, T> {
    type Output = MutexGuard<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.wait.poll(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(_)) => unsafe {
                mycelium_util::unreachable_unchecked!("`Mutex` never calls `WaitQueue::close`")
            },
            Poll::Pending => return Poll::Pending,
        }

        let data = this.mutex.data.get_mut();
        Poll::Ready(MutexGuard {
            _wake: WakeOnDrop(this.mutex),
            data,
        })
    }
}

// === impl MutexGuard ===

impl<'a, T> ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // safety: we are holding the lock
            &*self.data.deref()
        }
    }
}

impl<'a, T> ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // safety: we are holding the lock
            self.data.deref()
        }
    }
}

impl<'a, T> Drop for WakeOnDrop<'a, T> {
    fn drop(&mut self) {
        self.0.wait.wake()
    }
}
