//! An asynchronous [readers-writer lock].
//!
//! See the documentation for the [`RwLock`] type for details.
//!
//! [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
use super::semaphore::{self, Semaphore};
use crate::{
    blocking::RawMutex,
    loom::cell::{self, UnsafeCell},
    spin::Spinlock,
    util::fmt,
};
use core::ops::{Deref, DerefMut};

#[cfg(test)]
mod tests;

/// An asynchronous [readers-writer lock].
///
/// This type of lock protects shared data by allowing either multiple
/// concurrent readers (shared access), or a single writer (exclusive access) at
/// a given point in time. If the shared data must be modified, write access
/// must be acquired, preventing other threads from accessing the data while it
/// is being written, but multiple threads can read the shared data when it is
/// not being mutated.
///
/// This is in contrast to a [`Mutex`], which only ever allows a single
/// core/thread to access the shared data at any point in time. In some cases,
/// such as when a large number of readers need to access the shared data
/// without modifying it, using a `RwLock` can be more efficient than a
/// [`Mutex`].
///
/// # Usage
///
/// The type parameter `T` represents the data that this lock protects. It is
/// required that `T` satisfies [`Send`] to be shared across threads and
/// [`Sync`] to allow concurrent access through readers. The RAII guards
/// returned from the locking methods implement [`Deref`] (and [`DerefMut`]
/// for the `write` methods) to allow access to the content of the lock.
///
/// The [`read`] method acquires read access to the lock, returning a
/// [`RwLockReadGuard`]. If the lock is currently locked for write access, the
/// [`read`] method will wait until the write access completes before allowing
/// read access to the locked data.
///
/// The [`write`] method acquires write access to the lock, returning a
/// [`RwLockWriteGuard`], which implements [`DerefMut`]. If the lock is
/// currently locked for reading *or* writing, the [`write`] method will wait
/// until all current reads or the current write completes before allowing write
/// access to the locked data.
///
/// # Priority Policy
///
/// The priority policy of this lock is _fair_ (or [_write-preferring_]), in
/// order to ensure that readers cannot starve  writers. Fairness is ensured
/// using a first-in, first-out queue for the tasks awaiting the lock; if a task
/// that wishes to acquire the write lock is at the head of the queue, read
/// locks will not be given out until the write lock has  been released. This is
/// in contrast to the Rust standard library's [`std::sync::RwLock`], where the
/// priority policy is dependent on the operating system's implementation.
///
/// # Overriding the blocking mutex
///
/// This type uses a [blocking `Mutex`](crate::blocking::Mutex) internally to
/// synchronize access to its wait list. By default, this is a [`Spinlock`]. To
/// use an alternative [`RawMutex`] implementation, use the
/// [`new_with_raw_mutex`](Self::new_with_raw_mutex) constructor. See [the documentation
/// on overriding mutex
/// implementations](crate::blocking#overriding-mutex-implementations) for more
/// details.
///
/// Note that this type currently requires that the raw mutex implement
/// [`RawMutex`] rather than [`mutex_traits::ScopedRawMutex`]!
///
/// # Examples
///
/// ```
/// use maitake_sync::RwLock;
///
/// # async fn example() {
/// let lock = RwLock::new(5);
///
/// // many reader locks can be held at once
/// {
///     let r1 = lock.read().await;
///     let r2 = lock.read().await;
///     assert_eq!(*r1, 5);
///     assert_eq!(*r2, 5);
/// } // read locks are dropped at this point
///
/// // only one write lock may be held, however
/// {
///     let mut w = lock.write().await;
///     *w += 1;
///     assert_eq!(*w, 6);
/// } // write lock is dropped here
/// # }
/// # futures::executor::block_on(example());
/// ```
///
/// [`Mutex`]: crate::Mutex
/// [`read`]: Self::read
/// [`write`]: Self::write
/// [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
/// [_write-preferring_]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock#Priority_policies
/// [`std::sync::RwLock`]:
///     https://doc.rust-lang.org/stable/std/sync/struct.RwLock.html
pub struct RwLock<T: ?Sized, Lock: RawMutex = Spinlock> {
    /// The semaphore used to control access to `data`.
    ///
    /// To read `data`, a single permit must be acquired. To write to `data`,
    /// all the permits in the semaphore must be acquired.
    sem: Semaphore<Lock>,

    /// The data protected by the lock.
    data: UnsafeCell<T>,
}

/// [RAII] structure used to release the shared read access of a [`RwLock`] when
/// dropped.
///
/// The data protected by the [`RwLock`] can be accessed through this guard via
/// its [`Deref`](#impl-Deref) implementation.
///
/// This guard can be held across any `.await` point, as it implements
/// [`Send`].
///
/// This structure is created by the [`read`] and [`try_read`] methods on
/// [`RwLock`].
///
/// [RAII]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
/// [`read`]: RwLock::read
/// [`try_read`]: RwLock::try_read
#[must_use = "if unused, the `RwLock` will immediately unlock"]
pub struct RwLockReadGuard<'lock, T: ?Sized, Lock: RawMutex = Spinlock> {
    /// /!\ WARNING: semi-load-bearing drop order /!\
    ///
    /// This struct's field ordering is important for Loom tests; the `ConstPtr`
    /// must be dropped before the permit, as dropping the permit may wake
    /// another task that wants to access the cell, and Loom will still consider the data to
    /// be "accessed" until the `ConstPtr` is dropped.
    data: cell::ConstPtr<T>,
    _permit: semaphore::Permit<'lock, Lock>,
}

/// [RAII] structure used to release the exclusive write access of a [`RwLock`] when
/// dropped.
///
/// The data protected by the [`RwLock`] can be accessed through this guard via
/// its [`Deref`](#impl-Deref) and [`DerefMut`](#impl-Deref) implementations.
///
/// This guard can be held across any `.await` point, as it implements
/// [`Send`].
///
/// This structure is created by the [`write`] and [`try_write`] methods on
/// [`RwLock`].
///
/// [RAII]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
/// [`write`]: RwLock::write
/// [`try_write`]: RwLock::try_write
#[must_use = "if unused, the `RwLock` will immediately unlock"]
pub struct RwLockWriteGuard<'lock, T: ?Sized, Lock: RawMutex = Spinlock> {
    /// /!\ WARNING: semi-load-bearing drop order /!\
    ///
    /// This struct's field ordering is important for Loom tests; the `MutPtr`
    /// must be dropped before the permit, as dropping the permit may wake
    /// another task that wants to access the cell, and Loom will still consider
    /// the data to be "accessed mutably" until the `MutPtr` is dropped.
    data: cell::MutPtr<T>,
    _permit: semaphore::Permit<'lock, Lock>,
}

feature! {
    #![feature = "alloc"]
    mod owned;
    pub use self::owned::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard};
}

// === impl RwLock ===

impl<T> RwLock<T> {
    loom_const_fn! {
        /// Returns a new `RwLock` protecting the provided `data`, in an
        /// unlocked state.
        ///
        /// This constructor returns a `RwLock` that uses a [`Spinlock`] as the
        /// underlying blocking mutex implementation. To use an alternative
        /// [`RawMutex`] implementation, use the [`RwLock::new_with_raw_mutex`]
        /// constructor instead. See [the documentation on overriding mutex
        /// implementations](crate::blocking#overriding-mutex-implementations)
        /// for more details.
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake_sync::RwLock;
        ///
        /// let lock = RwLock::new(5);
        /// # drop(lock)
        /// ```
        ///
        /// Because this is a `const fn`, it may be used in `static`
        /// initializers:
        ///
        /// ```
        /// use maitake_sync::RwLock;
        ///
        /// static LOCK: RwLock<usize> = RwLock::new(5);
        /// ```
        #[must_use]
        pub fn new(data: T) -> Self {
            Self::new_with_raw_mutex(data, Spinlock::new())
        }
    }
}

impl<T, Lock: RawMutex> RwLock<T, Lock> {
    loom_const_fn! {
        /// Returns a new `RwLock` protecting the provided `data`, in an
        /// unlocked state, using the provided [`RawMutex`] implementation.
        ///
        /// This constructor allows a [`RwLock`] to be constructed with any type that
        /// implements [`RawMutex`] as the underlying raw blocking mutex
        /// implementation. See [the documentation on overriding mutex
        /// implementations](crate::blocking#overriding-mutex-implementations)
        /// for more details.
        #[must_use]
        pub fn new_with_raw_mutex(data: T, lock: Lock) -> Self {
            Self {
                sem: Semaphore::new_with_raw_mutex(Self::MAX_READERS, lock),
                data: UnsafeCell::new(data),
            }
        }
    }

    /// Consumes this `RwLock`, returning the guarded data.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: ?Sized, Lock: RawMutex> RwLock<T, Lock> {
    const MAX_READERS: usize = semaphore::MAX_PERMITS;

    /// Locks this `RwLock` with shared read access, causing the current task
    /// to yield until the lock has been acquired.
    ///
    /// If the lock is locked for write access, the calling task will yield and
    /// wait until there are no writers which  hold the lock. There may be other
    /// readers inside the lock when the task resumes.
    ///
    /// Note that under the [priority policy] of [`RwLock`], read locks are not
    /// granted until prior write locks, to prevent starvation. Therefore
    /// deadlock may occur if a read lock is held by the current task, a write
    /// lock attempt is made, and then a subsequent read lock attempt is made
    /// by the current task.
    ///
    /// Returns [an RAII guard] which will release this read access of the
    /// `RwLock`  when dropped.
    ///
    /// # Cancellation
    ///
    /// This method [uses a queue to fairly distribute locks][priority policy]
    /// in the order they  were requested. Cancelling a call to `read` results
    /// in the calling task losing its place in the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[tokio::main(flavor="current_thread")]
    /// # async fn test() {
    /// # // since we are targeting no-std, it makes more sense to use `alloc`
    /// # // in these examples, rather than `std`...but i don't want to make
    /// # // the tests actually `#![no_std]`...
    /// # use std as alloc;
    /// # use tokio::task;
    /// use maitake_sync::RwLock;
    /// use alloc::sync::Arc;
    ///
    /// let lock = Arc::new(RwLock::new(1));
    ///
    /// // hold the lock for reading in `main`.
    /// let n = lock
    ///     .try_read()
    ///     .expect("read lock must be acquired, as the lock is unlocked");
    /// assert_eq!(*n, 1);
    ///
    /// # let task2 =
    /// task::spawn({
    ///     let lock = lock.clone();
    ///     async move {
    ///         // While main has an active read lock, this task can acquire
    ///         // one too.
    ///         let n = lock.read().await;
    ///         assert_eq!(*n, 1);
    ///     }
    /// });
    ///
    /// # task2.await.unwrap();
    /// # }
    /// # test();
    /// ```
    ///
    /// [priority policy]: Self#priority-policy
    /// [an RAII guard]:
    pub async fn read(&self) -> RwLockReadGuard<'_, T, Lock> {
        let _permit = self
            .sem
            .acquire(1)
            .await
            .expect("RwLock semaphore should never be closed");
        RwLockReadGuard {
            data: self.data.get(),
            _permit,
        }
    }

    /// Locks this `RwLock` with exclusive write access, causing the current
    /// task to yield until the lock has been acquired.
    ///
    /// If other tasks are holding a read or write lock, the calling task will
    /// wait until the write lock or all read locks are released.
    ///
    /// Returns [an RAII guard] which will release the write access of this
    /// `RwLock` when dropped.
    ///
    /// # Cancellation
    ///
    /// This method [uses a queue to fairly distribute
    /// locks](Self#priority-policy) in the order they were requested.
    /// Cancelling a call to `write` results in the calling task losing its place
    /// in the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[tokio::main(flavor="current_thread")]
    /// # async fn test() {
    /// # // since we are targeting no-std, it makes more sense to use `alloc`
    /// # // in these examples, rather than `std`...but i don't want to make
    /// # // the tests actually `#![no_std]`...
    /// # use std as alloc;
    /// # use tokio::task;
    /// use maitake_sync::RwLock;
    /// use alloc::sync::Arc;
    ///
    /// let lock = Arc::new(RwLock::new(1));
    ///
    /// # let task =
    /// task::spawn(async move {
    ///     let mut guard = lock.write().await;
    ///     *guard += 1;
    /// });
    /// # task.await.unwrap()
    /// # }
    /// # test();
    /// ```
    pub async fn write(&self) -> RwLockWriteGuard<'_, T, Lock> {
        let _permit = self
            .sem
            .acquire(Self::MAX_READERS)
            .await
            .expect("RwLock semaphore should never be closed");
        RwLockWriteGuard {
            data: self.data.get_mut(),
            _permit,
        }
    }

    /// Attempts to acquire this `RwLock` for shared read access, without
    /// waiting.
    ///
    /// If the access couldn't be acquired immediately, this method returns
    /// [`None`] rather than waiting.
    ///
    /// Otherwise, [an RAII guard] is returned, which allows read access to the
    /// protected data and will release that access when dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use maitake_sync::RwLock;
    ///
    /// let lock = RwLock::new(1);
    ///
    /// let mut write_guard = lock
    ///     .try_write()
    ///     .expect("lock is unlocked, so write access should be acquired");
    /// *write_guard += 1;
    ///
    /// // because a write guard is held, we cannot acquire the read lock, so
    /// // this will return `None`.
    /// assert!(lock.try_read().is_none());
    /// ```
    ///
    /// [an RAII guard]: RwLockReadGuard
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T, Lock>> {
        match self.sem.try_acquire(1) {
            Ok(_permit) => Some(RwLockReadGuard {
                data: self.data.get(),
                _permit,
            }),
            Err(semaphore::TryAcquireError::InsufficientPermits) => None,
            Err(semaphore::TryAcquireError::Closed) => {
                unreachable!("RwLock semaphore should never be closed")
            }
        }
    }

    /// Attempts to acquire this `RwLock` for exclusive write access, without
    /// waiting.
    ///
    /// If the access couldn't be acquired immediately, this method returns
    /// [`None`] rather than waiting.
    ///
    /// Otherwise, [an RAII guard] is returned, which allows write access to the
    /// protected data and will release that access when dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use maitake_sync::RwLock;
    ///
    /// let lock = RwLock::new(1);
    ///
    /// let read_guard = lock
    ///     .try_read()
    ///     .expect("lock is unlocked, so read access should be acquired");
    /// assert_eq!(*read_guard, 1);
    ///
    /// // because a read guard is held, we cannot acquire the write lock, so
    /// // this will return `None`.
    /// assert!(lock.try_write().is_none());
    /// ```
    ///
    /// [an RAII guard]: RwLockWriteGuard
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T, Lock>> {
        match self.sem.try_acquire(Self::MAX_READERS) {
            Ok(_permit) => Some(RwLockWriteGuard {
                data: self.data.get_mut(),
                _permit,
            }),
            Err(semaphore::TryAcquireError::InsufficientPermits) => None,
            Err(semaphore::TryAcquireError::Closed) => {
                unreachable!("RwLock semaphore should never be closed")
            }
        }
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the `RwLock` mutably, no actual locking needs to
    /// take place -- the mutable borrow statically guarantees no locks exist.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut lock = maitake_sync::RwLock::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.try_read().unwrap(), 10);
    /// ```
    pub fn get_mut(&mut self) -> &mut T {
        unsafe {
            // Safety: since this call borrows the `RwLock` mutably, no actual
            // locking needs to take place -- the mutable borrow statically
            // guarantees no locks exist.
            self.data.with_mut(|data| &mut *data)
        }
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T, Lock> fmt::Debug for RwLock<T, Lock>
where
    T: ?Sized + fmt::Debug,
    Lock: RawMutex + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { sem, data: _ } = self;
        f.debug_struct("RwLock")
            .field("sem", sem)
            .field("data", &fmt::opt(&self.try_read()).or_else("<locked>"))
            .finish()
    }
}

// Safety: if `T` is `Send + Sync`, an `RwLock<T>` can safely be sent or shared
// between threads. If `T` wasn't `Send`, this would be unsafe, since the
// `RwLock` exposes access to the `T`.
unsafe impl<T, Lock> Send for RwLock<T, Lock>
where
    T: ?Sized + Send,
    Lock: RawMutex + Send,
{
}
unsafe impl<T, Lock> Sync for RwLock<T, Lock>
where
    T: ?Sized + Send + Sync,
    Lock: RawMutex + Sync,
{
}

// === impl RwLockReadGuard ===

impl<T: ?Sized, Lock: RawMutex> Deref for RwLockReadGuard<'_, T, Lock> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // safety: we are holding the semaphore permit that ensures the lock
            // cannot be accessed mutably.
            self.data.deref()
        }
    }
}

impl<T: ?Sized + fmt::Debug, Lock: RawMutex> fmt::Debug for RwLockReadGuard<'_, T, Lock> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

// Safety: A read guard can be shared or sent between threads as long as `T` is
// `Sync`. It can implement `Send` even if `T` does not implement `Send`, as
// long as `T` is `Sync`, because the read guard only permits borrowing the `T`.
unsafe impl<T, Lock> Send for RwLockReadGuard<'_, T, Lock>
where
    T: ?Sized + Sync,
    Lock: RawMutex + Sync,
{
}
unsafe impl<T, Lock> Sync for RwLockReadGuard<'_, T, Lock>
where
    T: ?Sized + Send + Sync,
    Lock: RawMutex + Sync,
{
}

// === impl RwLockWriteGuard ===

impl<T: ?Sized, Lock: RawMutex> Deref for RwLockWriteGuard<'_, T, Lock> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // safety: we are holding all the semaphore permits, so the data
            // inside the lock cannot be accessed by another thread.
            self.data.deref()
        }
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // safety: we are holding all the semaphore permits, so the data
            // inside the lock cannot be accessed by another thread.
            self.data.deref()
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

// Safety: Unlike the read guard, `T` must be both `Send` and `Sync` for the
// write guard to be `Send`, because the mutable access provided by the write
// guard can be used to `mem::replace` or `mem::take` the value, transferring
// ownership of it across threads.
unsafe impl<T> Send for RwLockWriteGuard<'_, T> where T: ?Sized + Send + Sync {}
unsafe impl<T> Sync for RwLockWriteGuard<'_, T> where T: ?Sized + Send + Sync {}
