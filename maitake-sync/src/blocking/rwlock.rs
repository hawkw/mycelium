/// A spinlock-based [readers-writer lock].
///
/// See the documentation for the [`RwLock`] type for more information.
///
/// [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
use crate::{
    loom::cell::{ConstPtr, MutPtr, UnsafeCell},
    spin::RwSpinlock,
    util::fmt,
};
use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

/// A spinlock-based [readers-writer lock].
///
/// This type of lock allows a number of readers or at most one writer at any
/// point in time. The write portion of this lock typically allows modification
/// of the underlying data (exclusive access) and the read portion of this lock
/// typically allows for read-only access (shared access).
///
/// In comparison, a [`blocking::Mutex`] does not distinguish between readers or writers
/// that acquire the lock, therefore blocking any threads waiting for the lock to
/// become available. An `RwLock` will allow any number of readers to acquire the
/// lock as long as a writer is not holding the lock.
///
/// # Fairness
///
/// This is *not* a fair reader-writer lock.
///
/// # Loom-specific behavior
///
/// When `cfg(loom)` is enabled, this mutex will use Loom's simulated atomics,
/// checked `UnsafeCell`, and simulated spin loop hints.
///
/// [`blocking::Mutex`]: crate::blocking::Mutex
/// [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
pub struct RwLock<T: ?Sized, Lock = RwSpinlock> {
    lock: Lock,
    data: UnsafeCell<T>,
}

/// An RAII implementation of a "scoped read lock" of a [`RwLock`]. When this
/// structure is dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the [`RwLock`] can be immutably accessed through this
/// guard via its [`Deref`] implementation.
///
/// This structure is created by the [`read`] and [`try_read`] methods on
/// [`RwLock`].
///
/// [`read`]: RwLock::read
/// [`try_read`]: RwLock::try_read
#[must_use = "if unused, the `RwLock` will immediately unlock"]
pub struct RwLockReadGuard<'lock, T: ?Sized, Lock: RawRwLock = RwSpinlock> {
    ptr: ConstPtr<T>,
    lock: &'lock Lock,
    _marker: PhantomData<Lock::GuardMarker>,
}

/// An RAII implementation of a "scoped write lock" of a [`RwLock`]. When this
/// structure is dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the [`RwLock`] can be mutably accessed through this
/// guard via its [`Deref`] and [`DerefMut`] implementations.
///
/// This structure is created by the [`write`] and [`try_write`] methods on
/// [`RwLock`].
///
/// [`write`]: RwLock::write
/// [`try_write`]: RwLock::try_write
#[must_use = "if unused, the `RwLock` will immediately unlock"]
pub struct RwLockWriteGuard<'lock, T: ?Sized, Lock: RawRwLock = RwSpinlock> {
    ptr: MutPtr<T>,
    lock: &'lock Lock,
    _marker: PhantomData<Lock::GuardMarker>,
}

/// Trait abstracting over blocking [`RwLock`] implementations (`maitake-sync`'s
/// version).
///
/// # Safety
///
/// Implementations of this trait must ensure that the `RwLock` is actually
/// exclusive: an exclusive lock can't be acquired while an exclusive or shared
/// lock exists, and a shared lock can't be acquire while an exclusive lock
/// exists.
pub unsafe trait RawRwLock {
    /// Marker type which determines whether a lock guard should be [`Send`].
    type GuardMarker;

    /// Acquires a shared lock, blocking the current thread/CPU core until it is
    /// able to do so.
    fn lock_shared(&self);

    /// Attempts to acquire a shared lock without blocking.
    fn try_lock_shared(&self) -> bool;

    /// Releases a shared lock.
    ///
    /// # Safety
    ///
    /// This method may only be called if a shared lock is held in the current context.
    unsafe fn unlock_shared(&self);

    /// Acquires an exclusive lock, blocking the current thread/CPU core until
    /// it is able to do so.
    fn lock_exclusive(&self);

    /// Attempts to acquire an exclusive lock without blocking.
    fn try_lock_exclusive(&self) -> bool;

    /// Releases an exclusive lock.
    ///
    /// # Safety
    ///
    /// This method may only be called if an exclusive lock is held in the
    /// current context.
    unsafe fn unlock_exclusive(&self);

    /// Returns `true` if this `RwLock` is currently locked in any way.
    fn is_locked(&self) -> bool;

    /// Returns `true` if this `RwLock` is currently locked exclusively.
    fn is_locked_exclusive(&self) -> bool;
}

#[cfg(feature = "lock_api")]
unsafe impl<T: lock_api::RawRwLock> RawRwLock for T {
    type GuardMarker = <T as lock_api::RawRwLock>::GuardMarker;

    #[inline]
    #[track_caller]
    fn lock_shared(&self) {
        lock_api::RawRwLock::lock_shared(self)
    }

    #[inline]
    #[track_caller]
    fn try_lock_shared(&self) -> bool {
        lock_api::RawRwLock::try_lock_shared(self)
    }

    #[inline]
    #[track_caller]
    unsafe fn unlock_shared(&self) {
        lock_api::RawRwLock::unlock_shared(self)
    }

    #[inline]
    #[track_caller]
    fn lock_exclusive(&self) {
        lock_api::RawRwLock::lock_exclusive(self)
    }

    #[inline]
    #[track_caller]
    fn try_lock_exclusive(&self) -> bool {
        lock_api::RawRwLock::try_lock_exclusive(self)
    }

    #[inline]
    #[track_caller]
    unsafe fn unlock_exclusive(&self) {
        lock_api::RawRwLock::unlock_exclusive(self)
    }

    #[inline]
    #[track_caller]
    fn is_locked(&self) -> bool {
        lock_api::RawRwLock::is_locked(self)
    }

    #[inline]
    #[track_caller]
    fn is_locked_exclusive(&self) -> bool {
        lock_api::RawRwLock::is_locked_exclusive(self)
    }
}

impl<T> RwLock<T> {
    loom_const_fn! {
        /// Creates a new, unlocked `RwLock<T>` protecting the provided `data`.
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake_sync::blocking::RwLock;
        ///
        /// let lock = RwLock::new(5);
        /// # drop(lock);
        /// ```
        #[must_use]
        pub fn new(data: T) -> Self {
            Self {
                lock: RwSpinlock::new(),
                data: UnsafeCell::new(data),
            }
        }
    }

    /// Returns the current number of readers holding a read lock.
    ///
    /// # Note
    ///
    /// This method is not synchronized with attempts to increment the reader
    /// count, and its value may become out of date as soon as it is read. This
    /// is **not** intended to be used for synchronization purposes! It is
    /// intended only for debugging purposes or for use as a heuristic.
    #[inline]
    #[must_use]
    pub fn reader_count(&self) -> usize {
        self.lock.reader_count()
    }
}

#[cfg(feature = "lock_api")]
impl<T, Lock: lock_api::RawRwLock> RwLock<T, Lock> {
    /// Creates a new instance of an `RwLock<T>` which is unlocked.
    pub const fn new_with_raw_mutex(data: T) -> Self {
        RwLock {
            data: UnsafeCell::new(data),
            lock: Lock::INIT,
        }
    }
}

impl<T: ?Sized, Lock: RawRwLock> RwLock<T, Lock> {
    fn read_guard(&self) -> RwLockReadGuard<'_, T, Lock> {
        RwLockReadGuard {
            ptr: self.data.get(),
            lock: &self.lock,
            _marker: PhantomData,
        }
    }

    fn write_guard(&self) -> RwLockWriteGuard<'_, T, Lock> {
        RwLockWriteGuard {
            ptr: self.data.get_mut(),
            lock: &self.lock,
            _marker: PhantomData,
        }
    }

    /// Locks this `RwLock` for shared read access, spinning until it can be
    /// acquired.
    ///
    /// The calling CPU core will spin (with an exponential backoff) until there
    /// are no more writers which hold the lock. There may be other readers
    /// currently inside the lock when this method returns. This method does not
    /// provide any guarantees with respect to the ordering of whether
    /// contentious readers or writers will acquire the lock first.
    ///
    /// Returns an RAII guard which will release this thread's shared access
    /// once it is dropped.
    #[cfg_attr(test, track_caller)]
    pub fn read(&self) -> RwLockReadGuard<'_, T, Lock> {
        self.lock.lock_shared();
        self.read_guard()
    }

    /// Attempts to acquire this `RwLock` for shared read access.
    ///
    /// If the access could not be granted at this time, this method returns
    /// [`None`]. Otherwise, [`Some`]`(`[`RwLockReadGuard`]`)` containing a RAII
    /// guard is returned. The shared access is released when it is dropped.
    ///
    /// This function does not spin.
    #[cfg_attr(test, track_caller)]
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T, Lock>> {
        if self.lock.try_lock_shared() {
            Some(self.read_guard())
        } else {
            None
        }
    }

    /// Locks this `RwLock` for exclusive write access, spinning until write
    /// access can be acquired.
    ///
    /// This function will not return while other writers or other readers
    /// currently have access to the lock.
    ///
    /// Returns an RAII guard which will drop the write access of this `RwLock`
    /// when dropped.
    #[cfg_attr(test, track_caller)]
    pub fn write(&self) -> RwLockWriteGuard<'_, T, Lock> {
        self.lock.lock_exclusive();
        self.write_guard()
    }

    /// Returns `true` if there is currently a writer holding a write lock.
    ///
    /// # Note
    ///
    /// This method is not synchronized its value may become out of date as soon
    /// as it is read. This is **not** intended to be used for synchronization
    /// purposes! It is intended only for debugging purposes or for use as a
    /// heuristic.
    #[inline]
    #[must_use]
    pub fn has_writer(&self) -> bool {
        self.lock.is_locked_exclusive()
    }

    /// Attempts to acquire this `RwLock` for exclusive write access.
    ///
    /// If the access could not be granted at this time, this method returns
    /// [`None`]. Otherwise, [`Some`]`(`[`RwLockWriteGuard`]`)` containing a
    /// RAII guard is returned. The write access is released when it is dropped.
    ///
    /// This function does not spin.
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T, Lock>> {
        if self.lock.try_lock_exclusive() {
            Some(self.write_guard())
        } else {
            None
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
    /// let mut lock = maitake_sync::blocking::RwLock::new(0);
    /// *lock.get_mut() = 10;
    /// assert_eq!(*lock.read(), 10);
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

impl<T, Lock: RawRwLock> RwLock<T, Lock> {
    /// Consumes this `RwLock`, returning the guarded data.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: Default, Lock: Default> Default for RwLock<T, Lock> {
    /// Creates a new `RwLock<T>`, with the `Default` value for T.
    fn default() -> RwLock<T, Lock> {
        RwLock {
            data: UnsafeCell::new(Default::default()),
            lock: Default::default(),
        }
    }
}

impl<T> From<T> for RwLock<T> {
    /// Creates a new instance of an `RwLock<T>` which is unlocked.
    /// This is equivalent to [`RwLock::new`].
    fn from(t: T) -> Self {
        RwLock::new(t)
    }
}

impl<T, Lock> fmt::Debug for RwLock<T, Lock>
where
    T: fmt::Debug,
    Lock: fmt::Debug + RawRwLock,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RwLock")
            .field(
                "data",
                &fmt::opt(&self.try_read()).or_else("<write locked>"),
            )
            .field("lock", &self.lock)
            .finish()
    }
}

unsafe impl<T: ?Sized + Send, Lock: Send> Send for RwLock<T, Lock> {}
unsafe impl<T: ?Sized + Send + Sync, Lock: Sync> Sync for RwLock<T, Lock> {}

// === impl RwLockReadGuard ===

impl<T: ?Sized, Lock: RawRwLock> Deref for RwLockReadGuard<'_, T, Lock> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // Safety: we are holding a read lock, so it is okay to dereference
            // the const pointer immutably.
            self.ptr.deref()
        }
    }
}

impl<T: ?Sized, R: ?Sized, Lock> AsRef<R> for RwLockReadGuard<'_, T, Lock>
where
    T: AsRef<R>,
    Lock: RawRwLock,
{
    #[inline]
    fn as_ref(&self) -> &R {
        self.deref().as_ref()
    }
}

impl<T: ?Sized, Lock: RawRwLock> Drop for RwLockReadGuard<'_, T, Lock> {
    #[inline]
    #[cfg_attr(test, track_caller)]
    fn drop(&mut self) {
        unsafe { self.lock.unlock_shared() }
    }
}

impl<T, Lock> fmt::Debug for RwLockReadGuard<'_, T, Lock>
where
    T: ?Sized + fmt::Debug,
    Lock: RawRwLock,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T, Lock> fmt::Display for RwLockReadGuard<'_, T, Lock>
where
    T: ?Sized + fmt::Display,
    Lock: RawRwLock,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

/// A [`RwLockReadGuard`] is [`Sync`] if both `T` and the `Lock` type parameter
/// are [`Sync`].
unsafe impl<T, Lock> Sync for RwLockReadGuard<'_, T, Lock>
where
    T: ?Sized + Sync,
    Lock: RawRwLock + Sync,
{
}
/// A [`RwLockReadGuard`] is [`Send`] if both `T` and the `Lock` type parameter
/// are [`Sync`], because sending a `RwLockReadGuard` is equivalent to sending a
/// `&(T, Lock)`.
///
/// Additionally, the `Lock` type's [`RawRwLock::GuardMarker`] must indicate
/// that the guard is [`Send`].
unsafe impl<T, Lock> Send for RwLockReadGuard<'_, T, Lock>
where
    T: ?Sized + Sync,
    Lock: RawRwLock + Sync,
    Lock::GuardMarker: Send,
{
}

// === impl RwLockWriteGuard ===

impl<T: ?Sized, Lock: RawRwLock> Deref for RwLockWriteGuard<'_, T, Lock> {
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

impl<T: ?Sized, Lock: RawRwLock> DerefMut for RwLockWriteGuard<'_, T, Lock> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // Safety: we are holding the lock, so it is okay to dereference the
            // mut pointer.
            self.ptr.deref()
        }
    }
}

impl<T: ?Sized, R: ?Sized, Lock> AsRef<R> for RwLockWriteGuard<'_, T, Lock>
where
    T: AsRef<R>,
    Lock: RawRwLock,
{
    #[inline]
    fn as_ref(&self) -> &R {
        self.deref().as_ref()
    }
}

impl<T: ?Sized, Lock: RawRwLock> Drop for RwLockWriteGuard<'_, T, Lock> {
    #[inline]
    #[cfg_attr(test, track_caller)]
    fn drop(&mut self) {
        unsafe { self.lock.unlock_exclusive() }
    }
}

impl<T, Lock> fmt::Debug for RwLockWriteGuard<'_, T, Lock>
where
    T: ?Sized + fmt::Debug,
    Lock: RawRwLock,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T, Lock> fmt::Display for RwLockWriteGuard<'_, T, Lock>
where
    T: ?Sized + fmt::Display,
    Lock: RawRwLock,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

/// A [`RwLockWriteGuard`] is only [`Send`] if `T` is [`Send`] and [`Sync`],
/// because it can be used to *move* a `T` across thread boundaries, as it
/// allows mutable access to the `T` that can be used with
/// [`core::mem::replace`] or [`core::mem::swap`].
unsafe impl<T, Lock> Send for RwLockWriteGuard<'_, T, Lock>
where
    T: ?Sized + Send + Sync,
    Lock: RawRwLock,
    Lock::GuardMarker: Send,
{
}

/// A [`RwLockWriteGuard`] is only [`Sync`] if `T` is [`Send`] and [`Sync`],
/// because it can be used to *move* a `T` across thread boundaries, as it
/// allows mutable access to the `T` that can be used with
/// [`core::mem::replace`] or [`core::mem::swap`].
unsafe impl<T, Lock> Sync for RwLockWriteGuard<'_, T, Lock>
where
    T: ?Sized + Send + Sync,
    Lock: RawRwLock,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loom::{self, sync::Arc, thread};

    #[test]
    fn write() {
        const WRITERS: usize = 2;

        loom::model(|| {
            let lock = Arc::new(RwLock::<usize>::new(0));
            let threads = (0..WRITERS)
                .map(|_| {
                    let lock = lock.clone();
                    thread::spawn(writer(lock))
                })
                .collect::<Vec<_>>();

            for thread in threads {
                thread.join().expect("writer thread mustn't panic");
            }

            let guard = lock.read();
            assert_eq!(*guard, WRITERS, "final state must equal number of writers");
        });
    }

    #[test]
    fn read_write() {
        // this hits loom's preemption bound with 2 writer threads.
        const WRITERS: usize = if cfg!(loom) { 1 } else { 2 };

        loom::model(|| {
            let lock = Arc::new(RwLock::<usize>::new(0));
            let w_threads = (0..WRITERS)
                .map(|_| {
                    let lock = lock.clone();
                    thread::spawn(writer(lock))
                })
                .collect::<Vec<_>>();

            {
                let guard = lock.read();
                assert!(*guard == 0 || *guard == 1 || *guard == 2);
            }

            for thread in w_threads {
                thread.join().expect("writer thread mustn't panic")
            }

            let guard = lock.read();
            assert_eq!(*guard, WRITERS, "final state must equal number of writers");
        });
    }

    fn writer(lock: Arc<RwLock<usize>>) -> impl FnOnce() {
        move || {
            test_debug!("trying to acquire write lock...");
            let mut guard = lock.write();
            test_debug!("got write lock!");
            *guard += 1;
        }
    }
}
