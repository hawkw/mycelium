use super::*;
use crate::semaphore;
use alloc::sync::Arc;

/// Owned [RAII] structure used to release the shared read access of a
/// [`RwLock`] when dropped.
///
/// This type is similar to the [`RwLockReadGuard`] type, but it is only
/// returned by an [`RwLock`] that is wrapped in an an [`Arc`]. Instead
/// of borrowing the [`RwLock`], this guard holds an [`Arc`] clone of
/// the [`RwLock`], incrementing its reference count. Therefore, this
/// type can outlive the [`RwLock`] that created it, and it is valid for
/// the `'static` lifetime. Beyond this, it is identical to the
/// [`RwLockReadGuard`] type.
///
/// The data protected by the [`RwLock`] can be accessed through this
/// guard via its [`Deref`](#impl-Deref) implementation.
///
/// This guard can be held across any `.await` point, as it implements
/// [`Send`].
///
/// This structure is created by the [`read_owned`] and
/// [`try_read_owned`] methods on [`Arc`]`<`[`RwLock`]`>`.
///
/// [RAII]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
/// [`read_owned`]: RwLock::read_owned
/// [`try_read_owned`]: RwLock::try_read_owned
#[must_use = "if unused, the `RwLock` will immediately unlock"]
pub struct OwnedRwLockReadGuard<T: ?Sized> {
    /// /!\ WARNING: semi-load-bearing drop order /!\
    ///
    /// This struct's field ordering is important for Loom tests; the `ConstPtr`
    /// must be dropped before the semaphore permit is released back to the
    /// semaphore, as this may wake another task that wants to mutably access
    /// the cell. However, Loom will still consider the data to be "immutably
    /// accessed" until the ConstPtr` is dropped, so we must drop the `ConstPtr`
    /// first.
    ///
    /// This isn't actually a bug in "real life", because we're not going to
    /// actually *read* the data through the `ConstPtr` in the guard's `Drop`
    /// impl, but Loom considers us to be "accessing" it as long as the
    /// `ConstPtr` exists.
    data: cell::ConstPtr<T>,
    _lock: AddPermits<1, T>,
}

/// Owned [RAII] structure used to release the exclusive write access of a
/// [`RwLock`] when dropped.
///
/// This type is similar to the [`RwLockWriteGuard`] type, but it is
/// only returned by an [`RwLock`] that is wrapped in an an [`Arc`].
/// Instead of borrowing the [`RwLock`], this guard holds an [`Arc`]
/// clone of the [`RwLock`], incrementing its reference count.
/// Therefore, this type can outlive the [`RwLock`] that created it, and
/// it is valid for the `'static` lifetime. Beyond this, is identical to
/// the [`RwLockWriteGuard`] type.
///
/// The data protected by the [`RwLock`] can be accessed through this
/// guard via its [`Deref`](#impl-Deref) and [`DerefMut`](#impl-Deref)
/// implementations.
///
/// This guard can be held across any `.await` point, as it implements
/// [`Send`].
///
/// This structure is created by the [`read_owned`] and
/// [`try_read_owned`] methods on [`Arc`]`<`[`RwLock`]`>`.
///
/// [RAII]: https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html
/// [`read_owned`]: RwLock::read_owned
/// [`try_read_owned`]: RwLock::try_read_owned
#[must_use = "if unused, the `RwLock` will immediately unlock"]
pub struct OwnedRwLockWriteGuard<T: ?Sized> {
    /// /!\ WARNING: semi-load-bearing drop order /!\
    ///
    /// This struct's field ordering is important for Loom tests; the `MutPtr`
    /// must be dropped before the semaphore permits are released back to the
    /// semaphore, as this may wake another task that wants to access the cell.
    /// However, Loom will still consider the data to be "mutably accessed"
    /// until the `MutPtr` is dropped, so we must drop the `MutPtr` first.
    ///
    /// This isn't actually a bug in "real life", because we're not going to
    /// actually read or write the data through the `MutPtr` in the guard's
    /// `Drop` impl, but Loom considers us to be "accessing" it as long as the
    /// `MutPtr` exists.
    data: cell::MutPtr<T>,
    _lock: AddPermits<{ semaphore::MAX_PERMITS }, T>,
}

/// A wrapper around an `RwLock` `Arc` clone that releases a fixed number of
/// permits when it's dropped.
///
/// This is factored out to a separate type to ensure that it's dropped *after*
/// the `MutPtr`/`ConstPtr`s are dropped, to placate `loom`.
struct AddPermits<const PERMITS: usize, T: ?Sized>(Arc<RwLock<T>>);

// === impl RwLock ===

impl<T: ?Sized> RwLock<T> {
    /// Locks this `RwLock` with shared read access, returning an [owned RAII
    /// guard][guard].
    ///
    /// This method is identical to [`RwLock::read`], execept that it requires
    /// the `RwLock` to be wrapped in an [`Arc`], and returns an
    /// [`OwnedRwLockReadGuard`] that clones the [`Arc`] rather than
    /// borrowing the lock. Therefore, the returned guard is valid for the
    /// `'static` lifetime.
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
    /// Returns [an RAII guard][guard] which will release this read access of the
    /// `RwLock` when dropped.
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
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// # // since we are targeting no-std, it makes more sense to use `alloc`
    /// # // in these examples, rather than `std`...but i don't want to make
    /// # // the tests actually `#![no_std]`...
    /// # use std as alloc;
    /// use maitake_sync::RwLock;
    /// use alloc::sync::Arc;
    ///
    /// let lock = Arc::new(RwLock::new(1));
    /// // hold the lock for reading in `main`.
    /// let n = lock
    ///     .try_read()
    ///     .expect("read lock must be acquired, as the lock is unlocked");
    /// assert_eq!(*n, 1);
    ///
    /// # let task =
    /// task::spawn({
    ///     let lock = lock.clone();
    ///     async move {
    ///         // While main has an active read lock, this task can acquire
    ///         // one too.
    ///         let n = lock.read_owned().await;
    ///         assert_eq!(*n, 1);
    ///     }
    /// });
    /// # task.await.unwrap();
    /// # }
    /// # test();
    /// ```
    ///
    /// [priority policy]: Self#priority-policy
    /// [guard]: OwnedRwLockReadGuard
    pub async fn read_owned(self: &Arc<Self>) -> OwnedRwLockReadGuard<T> {
        let guard = self.read().await;
        OwnedRwLockReadGuard::from_borrowed(self.clone(), guard)
    }

    /// Locks this `RwLock` with exclusive write access,returning an [owned RAII
    /// guard][guard].
    ///
    /// This method is identical to [`RwLock::write`], execept that it requires
    /// the `RwLock` to be wrapped in an [`Arc`], and returns an
    /// [`OwnedRwLockWriteGuard`] that clones the [`Arc`] rather than
    /// borrowing the lock. Therefore, the returned guard is valid for the
    /// `'static` lifetime.
    ///
    /// # Returns
    ///
    /// If other tasks are holding a read or write lock, the calling task will
    /// wait until the write lock or all read locks are released.
    ///
    /// Returns [an RAII guard][guard] which will release the write access of this
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
    /// # use tokio::task;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn test() {
    /// # // since we are targeting no-std, it makes more sense to use `alloc`
    /// # // in these examples, rather than `std`...but i don't want to make
    /// # // the tests actually `#![no_std]`...
    /// # use std as alloc;
    /// use maitake_sync::RwLock;
    /// use alloc::sync::Arc;
    ///
    /// let lock = Arc::new(RwLock::new(1));
    ///
    /// # let task =
    /// task::spawn(async move {
    ///     let mut guard = lock.write_owned().await;
    ///     *guard += 1;
    /// });
    /// # task.await.unwrap();
    /// # }
    /// # test();
    /// ```
    ///
    /// [guard]: OwnedRwLockWriteGuard
    pub async fn write_owned(self: &Arc<Self>) -> OwnedRwLockWriteGuard<T> {
        let guard = self.write().await;
        OwnedRwLockWriteGuard::from_borrowed(self.clone(), guard)
    }

    /// Attempts to acquire this `RwLock` for shared read access, without
    /// waiting, and returning an [owned RAII guard][guard].
    ///
    /// This method is identical to [`RwLock::try_read`], execept that it requires
    /// the `RwLock` to be wrapped in an [`Arc`], and returns an
    /// [`OwnedRwLockReadGuard`] that clones the [`Arc`] rather than
    /// borrowing the lock. Therefore, the returned guard is valid for the
    /// `'static` lifetime.
    ///
    /// # Returns
    ///
    /// If the access couldn't be acquired immediately, this method returns
    /// [`None`] rather than waiting.
    ///
    /// Otherwise, [an RAII guard][guard] is returned, which allows read access to the
    /// protected data and will release that access when dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # // since we are targeting no-std, it makes more sense to use `alloc`
    /// # // in these examples, rather than `std`...but i don't want to make
    /// # // the tests actually `#![no_std]`...
    /// # use std as alloc;
    /// use maitake_sync::RwLock;
    /// use alloc::sync::Arc;
    ///
    /// let lock = Arc::new(RwLock::new(1));
    ///
    /// let mut write_guard = lock
    ///     .try_write()
    ///     .expect("lock is unlocked, so write access should be acquired");
    /// *write_guard += 1;
    ///
    /// // because a write guard is held, we cannot acquire the read lock, so
    /// // this will return `None`.
    /// assert!(lock.try_read_owned().is_none());
    /// # }
    /// ```
    ///
    /// [guard]: OwnedRwLockReadGuard
    pub fn try_read_owned(self: &Arc<Self>) -> Option<OwnedRwLockReadGuard<T>> {
        self.try_read()
            .map(|guard| OwnedRwLockReadGuard::from_borrowed(self.clone(), guard))
    }

    /// Attempts to acquire this `RwLock` for exclusive write access, without
    /// waiting, and returning an [owned RAII guard][guard].
    ///
    /// This method is identical to [`RwLock::try_write`], execept that it requires
    /// the `RwLock` to be wrapped in an [`Arc`], and returns an
    /// [`OwnedRwLockWriteGuard`] that clones the [`Arc`] rather than
    /// borrowing the lock. Therefore, the returned guard is valid for the
    /// `'static` lifetime.
    ///
    /// # Returns
    ///
    /// If the access couldn't be acquired immediately, this method returns
    /// [`None`] rather than waiting.
    ///
    /// Otherwise, [an RAII guard][guard] is returned, which allows write access to the
    /// protected data and will release that access when dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # // since we are targeting no-std, it makes more sense to use `alloc`
    /// # // in these examples, rather than `std`...but i don't want to make
    /// # // the tests actually `#![no_std]`...
    /// # use std as alloc;
    /// use maitake_sync::RwLock;
    /// use alloc::sync::Arc;
    ///
    /// let lock = Arc::new(RwLock::new(1));
    ///
    /// let read_guard = lock
    ///     .try_read()
    ///     .expect("lock is unlocked, so read access should be acquired");
    /// assert_eq!(*read_guard, 1);
    ///
    /// // because a read guard is held, we cannot acquire the write lock, so
    /// // this will return `None`.
    /// assert!(lock.try_write_owned().is_none());
    /// # }
    /// ```
    ///
    /// [guard]: OwnedRwLockWriteGuard
    pub fn try_write_owned(self: &Arc<Self>) -> Option<OwnedRwLockWriteGuard<T>> {
        self.try_write()
            .map(|guard| OwnedRwLockWriteGuard::from_borrowed(self.clone(), guard))
    }
}

// === impl OwnedRwLockReadGuard ===

impl<T: ?Sized> OwnedRwLockReadGuard<T> {
    fn from_borrowed(
        lock: Arc<RwLock<T>>,
        RwLockReadGuard { data, _permit }: RwLockReadGuard<'_, T>,
    ) -> Self {
        // forget the semaphore permit, as it has a lifetime tied to the
        // borrowed semaphore. we'll manually release the permit in
        // `OwnedRwLockReadGuard`'s `Drop` impl.
        _permit.forget();
        Self {
            _lock: AddPermits(lock),
            data,
        }
    }
}

impl<T: ?Sized> Deref for OwnedRwLockReadGuard<T> {
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

impl<T: ?Sized + fmt::Debug> fmt::Debug for OwnedRwLockReadGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

// Safety: A read guard can be shared or sent between threads as long as `T` is
// `Sync`. It can implement `Send` even if `T` does not implement `Send`, as
// long as `T` is `Sync`, because the read guard only permits borrowing the `T`.
unsafe impl<T> Send for OwnedRwLockReadGuard<T> where T: ?Sized + Sync {}
unsafe impl<T> Sync for OwnedRwLockReadGuard<T> where T: ?Sized + Send + Sync {}

// === impl OwnedRwLockWriteGuard ===

impl<T: ?Sized> OwnedRwLockWriteGuard<T> {
    fn from_borrowed(
        lock: Arc<RwLock<T>>,
        RwLockWriteGuard { data, _permit }: RwLockWriteGuard<'_, T>,
    ) -> Self {
        // forget the semaphore permit, as it has a lifetime tied to the
        // borrowed semaphore. we'll manually release the permit in
        // `OwnedRwLockWriteGuard`'s `Drop` impl.
        _permit.forget();
        Self {
            _lock: AddPermits(lock),
            data,
        }
    }
}

impl<T: ?Sized> Deref for OwnedRwLockWriteGuard<T> {
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

impl<T: ?Sized> DerefMut for OwnedRwLockWriteGuard<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // safety: we are holding all the semaphore permits, so the data
            // inside the lock cannot be accessed by another thread.
            self.data.deref()
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for OwnedRwLockWriteGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

// Safety: Unlike the read guard, `T` must be both `Send` and `Sync` for the
// write guard to be `Send`, because the mutable access provided by the write
// guard can be used to `mem::replace` or `mem::take` the value, transferring
// ownership of it across threads.
unsafe impl<T> Send for OwnedRwLockWriteGuard<T> where T: ?Sized + Send + Sync {}
unsafe impl<T> Sync for OwnedRwLockWriteGuard<T> where T: ?Sized + Send + Sync {}

// === impl AddPermits ===

impl<const PERMITS: usize, T: ?Sized> Drop for AddPermits<PERMITS, T> {
    fn drop(&mut self) {
        self.0.sem.add_permits(PERMITS);
    }
}
