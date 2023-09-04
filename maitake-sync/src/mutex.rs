//! An asynchronous [mutual exclusion lock].
//!
//! See the documentation on the [`Mutex`] type for details.
//!
//! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
use crate::{
    loom::cell::{MutPtr, UnsafeCell},
    util::fmt,
    wait_queue::{self, WaitQueue},
};
use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};
use pin_project::pin_project;

#[cfg(test)]
mod tests;

/// An asynchronous [mutual exclusion lock][mutex] for protecting shared data.
///
/// The data can only be accessed through the [RAII guards] returned
/// from [`lock`] and [`try_lock`], which guarantees that the data is only ever
/// accessed when the mutex is locked.
///
/// # Comparison With Other Mutices
///
/// This is an *asynchronous* mutex. When the shared data is locked, the
/// [`lock`] method will wait by causing the current [task] to yield until the
/// shared data is available. This is in contrast to *blocking* mutices, such as
/// [`std::sync::Mutex`], which wait by blocking the current thread[^1], or
/// *spinlock* based mutices, such as [`spin::Mutex`], which wait by spinning
/// in a busy loop.
///
/// The [`futures-util`] crate also provides an implementation of an asynchronous
/// mutex, [`futures_util::lock::Mutex`]. However, this mutex requires the Rust
/// standard library, and is thus unsuitable for use in environments where the
/// standard library is unavailable. In addition, the `futures-util` mutex
/// requires an additional allocation for every task that is waiting to acquire
/// the lock, while `maitake`'s mutex is based on an [intrusive linked list],
/// and therefore can be used without allocation[^2]. This makes `maitake`'s
/// mutex suitable for environments where heap allocations must be minimized or
/// cannot be used at all.
///
/// In addition, this is a [fairly queued] mutex. This means that the lock is
/// always acquired in a first-in, first-out order &mdash; if a task acquires
/// and then releases the lock, and then wishes to acquire the lock again, it
/// will not acquire the lock until every other task ahead of it in the queue
/// has had a chance to lock the shared data. Again, this is in contrast to
/// [`std::sync::Mutex`], where fairness depends on the underlying OS' locking
/// primitives; and [`spin::Mutex`] and [`futures_util::lock::Mutex`], which
/// will never guarantee fairness.
///
/// Finally, this mutex does not implement [poisoning][^3], unlike
/// [`std::sync::Mutex`].
///
/// [^1]: And therefore require an operating system to manage threading.
///
/// [^2]: The [tasks](core::task) themselves must, of course, be stored
///     somewhere, but this need not be a heap allocation in systems with a
///     fixed set of statically-allocated tasks. And, when tasks *are*
///     heap-allocated, these allocations [need not be provided by
///     `liballoc`][storage].
///
/// [^3]: In fact, this mutex _cannot_ implement poisoning, as poisoning
///     requires support for unwinding, and [`maitake` assumes that panics are
///     invariably fatal][no-unwinding].
///
/// [mutex]: https://en.wikipedia.org/wiki/Mutual_exclusion
/// [RAII guards]: MutexGuard
/// [`lock`]: Self::lock
/// [`try_lock`]: Self::try_lock
/// [task]: core::task
/// [fairly queued]: https://en.wikipedia.org/wiki/Unbounded_nondeterminism#Fairness
/// [`std::sync::Mutex`]: https://doc.rust-lang.org/stable/std/sync/struct.Mutex.html
/// [`spin::Mutex`]: crate::spin::Mutex
/// [`futures-util`]: https://crates.io/crate/futures-util
/// [`futures_util::lock::Mutex`]: https://docs.rs/futures-util/latest/futures_util/lock/struct.Mutex.html
/// [intrusive linked list]: crate::WaitQueue#implementation-notes
/// [poisoning]: https://doc.rust-lang.org/stable/std/sync/struct.Mutex.html#poisoning
// for some reason, intra-doc links don't work in footnotes?
/// [storage]: https://mycelium.elizas.website/maitake/task/trait.Storage.html
/// [no-unwinding]: https://mycelium.elizas.website/maitake/index.html#maitake-does-not-support-unwinding

pub struct Mutex<T: ?Sized> {
    wait: WaitQueue,
    data: UnsafeCell<T>,
}

/// An [RAII] implementation of a "scoped lock" of a [`Mutex`]. When this
/// structure is dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the mutex can be accessed through this guard via its
/// [`Deref`](#impl-Deref) and [`DerefMut`](#impl-Deref) implementations.
///
/// This guard can be held across any `.await` point, as it implements
/// [`Send`].
///
/// This structure is created by the [`lock`] and [`try_lock`] methods on
/// [`Mutex`].
///
/// [`lock`]: Mutex::lock
/// [`try_lock`]: Mutex::try_lock
/// [RAII]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
#[must_use = "if unused, the Mutex will immediately unlock"]
pub struct MutexGuard<'a, T: ?Sized> {
    /// /!\ WARNING: semi-load-bearing drop order /!\
    ///
    /// This struct's field ordering is important.
    data: MutPtr<T>,
    _wake: WakeOnDrop<'a, T>,
}

/// A [future] returned by the [`Mutex::lock`] method.
///
/// [future]: core::future::Future
#[must_use = "futures do nothing unless `.await`ed or `poll`ed"]
#[pin_project]
#[derive(Debug)]
pub struct Lock<'a, T: ?Sized> {
    #[pin]
    wait: wait_queue::Wait<'a>,
    mutex: &'a Mutex<T>,
}

/// This is used in order to ensure that the wakeup is performed only *after*
/// the data ptr is dropped, in order to keep `loom` happy.
struct WakeOnDrop<'a, T: ?Sized>(&'a Mutex<T>);

// === impl Mutex ===

impl<T> Mutex<T> {
    loom_const_fn! {
        /// Returns a new `Mutex` protecting the provided `data`.
        ///
        /// The returned `Mutex` will be in the unlocked state and is ready for
        /// use.
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake_sync::Mutex;
        ///
        /// let lock = Mutex::new(42);
        /// ```
        ///
        /// As this is a `const fn`, it may be used in a `static` initializer:
        /// ```
        /// use maitake_sync::Mutex;
        ///
        /// static GLOBAL_LOCK: Mutex<usize> = Mutex::new(42);
        /// ```
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
}

impl<T: ?Sized> Mutex<T> {
    /// Locks this mutex.
    ///
    /// This returns a [`Lock`] future that will wait until no other task is
    /// accessing the shared data. If the shared data is not locked, this future
    /// will complete immediately. When the lock has been acquired, this future
    /// will return a [`MutexGuard`].
    ///
    /// # Examples
    ///
    /// ```
    /// use maitake_sync::Mutex;
    ///
    /// async fn example() {
    ///     let mutex = Mutex::new(1);
    ///
    ///     let mut guard = mutex.lock().await;
    ///     *guard = 2;
    /// }
    /// ```
    pub fn lock(&self) -> Lock<'_, T> {
        Lock {
            wait: self.wait.wait(),
            mutex: self,
        }
    }

    /// Attempts to lock the mutex without waiting, returning `None` if the
    /// mutex is already locked locked.
    ///
    /// # Returns
    ///
    /// - `Some(`[`MutexGuard`])` if the mutex was not already locked
    /// - `None` if the mutex is currently locked and locking it would require
    ///   waiting
    ///
    /// # Examples
    ///
    /// ```
    /// use maitake_sync::Mutex;
    /// # async fn dox() -> Option<()> {
    ///
    /// let mutex = Mutex::new(1);
    ///
    /// let n = mutex.try_lock()?;
    /// assert_eq!(*n, 1);
    /// # Some(())
    /// # }
    /// ```
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

impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { data: _, wait } = self;
        f.debug_struct("Mutex")
            .field("data", &fmt::opt(&self.try_lock()).or_else("<locked>"))
            .field("wait", wait)
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

impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // safety: we are holding the lock
            &*self.data.deref()
        }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // safety: we are holding the lock
            self.data.deref()
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

unsafe impl<T: ?Sized> Send for MutexGuard<'_, T> where T: Send {}
unsafe impl<T: ?Sized> Sync for MutexGuard<'_, T> where T: Send + Sync {}

impl<'a, T: ?Sized> Drop for WakeOnDrop<'a, T> {
    fn drop(&mut self) {
        self.0.wait.wake()
    }
}

feature! {
    #![feature = "alloc"]

    use alloc::sync::Arc;

    /// An [RAII] implementation of a "scoped lock" of a [`Mutex`]. When this
    /// structure is dropped (falls out of scope), the lock will be unlocked.
    ///
    /// This type is similar to the [`MutexGuard`] type, but it is only returned
    /// by a [`Mutex`] that is wrapped in an an [`Arc`]. Instead of borrowing
    /// the [`Mutex`], this guard holds an [`Arc`] clone of the [`Mutex`],
    /// incrementing its reference count. Therefore, this type can outlive the
    /// [`Mutex`] that created it, and it is valid for the `'static` lifetime.
    ///
    /// The data protected by the mutex can be accessed through this guard via its
    /// [`Deref`](#impl-Deref) and [`DerefMut`](#impl-Deref) implementations.
    ///
    /// This guard can be held across any `.await` point, as it implements
    /// [`Send`].
    ///
    /// This structure is created by the [`lock_owned`] and [`try_lock_owned`]
    /// methods on  [`Mutex`].
    ///
    /// [`lock_owned`]: Mutex::lock_owned
    /// [`try_lock_owned`]: Mutex::try_lock_owned
    /// [RAII]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
    #[must_use = "if unused, the Mutex will immediately unlock"]
    pub struct OwnedMutexGuard<T: ?Sized> {
        /// /!\ WARNING: semi-load-bearing drop order /!\
        ///
        /// This struct's field ordering is important.
        data: MutPtr<T>,
        _wake: WakeArcOnDrop<T>,
    }

    impl<T: ?Sized> Mutex<T> {

        /// Locks this mutex, returning an [owned RAII guard][`OwnedMutexGuard`].
        ///
        /// This function will that will wait until no other task is
        /// accessing the shared data. If the shared data is not locked, this future
        /// will complete immediately. When the lock has been acquired, this future
        /// will return a [`OwnedMutexGuard`].
        ///
        /// This method is similar to [`Mutex::lock`], except that (rather
        /// than borrowing the [`Mutex`]) the returned  guard owns an [`Arc`]
        /// clone, incrememting its reference count. Therefore, this method is
        /// only available when the [`Mutex`] is wrapped in an [`Arc`], and the
        /// returned guard is valid for the `'static` lifetime.
        ///
        /// # Examples
        ///
        /// ```
        /// # // since we are targeting no-std, it makes more sense to use `alloc`
        /// # // in these examples, rather than `std`...but i don't want to make
        /// # // the tests actually `#![no_std]`...
        /// # use std as alloc;
        /// use maitake_sync::Mutex;
        /// use alloc::sync::Arc;
        ///
        /// # fn main() {
        /// async fn example() {
        ///     let mutex = Arc::new(Mutex::new(1));
        ///
        ///     let mut guard = mutex.clone().lock_owned().await;
        ///     *guard = 2;
        ///     # drop(mutex);
        /// }
        /// # }
        /// ```
        pub async fn lock_owned(self: Arc<Self>) -> OwnedMutexGuard<T> {
            self.wait.wait().await.unwrap();
            unsafe {
                // safety: we have just acquired the lock
                self.owned_guard()
            }
        }

        /// Attempts this mutex without waiting, returning an [owned RAII
        /// guard][`OwnedMutexGuard`], or `Err` if the mutex is already locked.
        ///
        /// This method is similar to [`Mutex::try_lock`], except that (rather
        /// than borrowing the [`Mutex`]) the returned guard owns an [`Arc`]
        /// clone, incrememting its reference count. Therefore, this method is
        /// only available when the [`Mutex`] is wrapped in an [`Arc`], and the
        /// returned guard is valid for the `'static` lifetime.
        ///
        /// # Returns
        ///
        /// - `Ok(`[`OwnedMutexGuard`])` if the mutex was not already locked
        /// - `Err(Arc<Mutex<T>>)` if the mutex is currently locked and locking
        ///   it would require waiting.
        ///
        ///   This returns an [`Err`] rather than [`None`] so that the same
        ///   [`Arc`] clone may be reused (such as by calling `try_lock_owned`
        ///   again) without having to decrement and increment the reference
        ///   count again.
        ///
        /// # Examples
        ///
        /// ```
        /// # // since we are targeting no-std, it makes more sense to use `alloc`
        /// # // in these examples, rather than `std`...but i don't want to make
        /// # // the tests actually `#![no_std]`...
        /// # use std as alloc;
        /// use maitake_sync::Mutex;
        /// use alloc::sync::Arc;
        ///
        /// # fn main() {
        /// let mutex = Arc::new(Mutex::new(1));
        ///
        /// if let Ok(guard) = mutex.clone().try_lock_owned() {
        ///     assert_eq!(*guard, 1);
        /// }
        /// # }
        /// ```
        pub fn try_lock_owned(self: Arc<Self>) -> Result<OwnedMutexGuard<T>, Arc<Self>> {
            match self.wait.try_wait() {
                Poll::Pending => Err(self),
                Poll::Ready(Ok(_)) => Ok(unsafe {
                    // safety: we have just acquired the lock
                    self.owned_guard()
                }),
                Poll::Ready(Err(_)) => unsafe {
                    unreachable_unchecked!("`Mutex` never calls `WaitQueue::close`")
                },
            }
        }

        /// Constructs a new `OwnedMutexGuard` for this `Mutex`.
        ///
        /// # Safety
        ///
        /// This may only be called once a lock has been acquired.
        unsafe fn owned_guard(self: Arc<Self>) -> OwnedMutexGuard<T> {
            let data = self.data.get_mut();
            OwnedMutexGuard {
                _wake: WakeArcOnDrop(self),
                data,
            }
        }
    }

    struct WakeArcOnDrop<T: ?Sized>(Arc<Mutex<T>>);

    // === impl OwnedMutexGuard ===

    impl<T: ?Sized> Deref for OwnedMutexGuard<T> {
        type Target = T;

        #[inline]
        fn deref(&self) -> &Self::Target {
            unsafe {
                // safety: we are holding the lock
                &*self.data.deref()
            }
        }
    }

    impl<T: ?Sized> DerefMut for OwnedMutexGuard<T> {
        #[inline]
        fn deref_mut(&mut self) -> &mut Self::Target {
            unsafe {
                // safety: we are holding the lock
                self.data.deref()
            }
        }
    }

    impl<T: ?Sized + fmt::Debug> fmt::Debug for OwnedMutexGuard<T> {
        #[inline]
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.deref().fmt(f)
        }
    }

    unsafe impl<T: ?Sized> Send for OwnedMutexGuard<T> where T: Send {}
    unsafe impl<T: ?Sized> Sync for OwnedMutexGuard<T> where T: Send + Sync {}

    impl<T: ?Sized> Drop for WakeArcOnDrop<T> {
        fn drop(&mut self) {
            self.0.wait.wake()
        }
    }
}
