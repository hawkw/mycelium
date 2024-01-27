use crate::{
    loom::{
        cell::{ConstPtr, MutPtr, UnsafeCell},
        sync::atomic::{AtomicUsize, Ordering::*},
    },
    util::Backoff,
};
use core::{
    fmt,
    ops::{Deref, DerefMut},
};

/// A spinlock-based [readers-writer lock].
///
/// This type of lock allows a number of readers or at most one writer at any
/// point in time. The write portion of this lock typically allows modification
/// of the underlying data (exclusive access) and the read portion of this lock
/// typically allows for read-only access (shared access).
///
/// In comparison, a [`spin::Mutex`] does not distinguish between readers or writers
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
/// [`spin::Mutex`]: crate::spin::Mutex
/// [readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
pub struct RwLock<T: ?Sized> {
    state: AtomicUsize,
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
pub struct RwLockReadGuard<'lock, T: ?Sized> {
    ptr: ConstPtr<T>,
    state: &'lock AtomicUsize,
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
pub struct RwLockWriteGuard<'lock, T: ?Sized> {
    ptr: MutPtr<T>,
    state: &'lock AtomicUsize,
}

const UNLOCKED: usize = 0;
const WRITER: usize = 1 << 0;
const READER: usize = 1 << 1;

impl<T> RwLock<T> {
    loom_const_fn! {
        /// Creates a new, unlocked `RwLock<T>` protecting the provided `data`.
        ///
        /// # Examples
        ///
        /// ```
        /// use maitake_sync::spin::RwLock;
        ///
        /// let lock = RwLock::new(5);
        /// # drop(lock);
        /// ```
        #[must_use]
        pub fn new(data: T) -> Self {
            Self {
                state: AtomicUsize::new(0),
                data: UnsafeCell::new(data),
            }
        }
    }
}

impl<T: ?Sized> RwLock<T> {
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
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        let mut backoff = Backoff::new();
        loop {
            if let Some(guard) = self.try_read() {
                return guard;
            }
            backoff.spin();
        }
    }

    /// Attempts to acquire this `RwLock` for shared read access.
    ///
    /// If the access could not be granted at this time, this method returns
    /// [`None`]. Otherwise, [`Some`]`(`[`RwLockReadGuard`]`)` containing a RAII
    /// guard is returned. The shared access is released when it is dropped.
    ///
    /// This function does not spin.
    #[cfg_attr(test, track_caller)]
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, T>> {
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
            return None;
        }

        Some(RwLockReadGuard {
            ptr: self.data.get(),
            state: &self.state,
        })
    }

    /// Locks this `RwLock` for exclusive write access, spinning until write
    /// access can be acquired.
    ///
    /// This function will not return while other writers or other readers
    /// currently have access to the lock.
    ///
    /// Returns an RAII guard which will drop the write access of this `RwLock`
    /// when dropped.
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        let mut backoff = Backoff::new();

        // Wait for the lock to become available and set the `WRITER` bit.
        //
        // Note that, unlike the `read` method, we don't use the `try_write`
        // method here, as we would like to use `compare_exchange_weak` to allow
        // spurious failures for improved performance. The `try_write` method
        // cannot use `compare_exchange_weak`, because it will never retry, and
        // a spurious failure means we would incorrectly fail to lock the RwLock
        // when we should have successfully locked it.
        while test_dbg!(self
            .state
            .compare_exchange_weak(UNLOCKED, WRITER, Acquire, Relaxed))
        .is_err()
        {
            test_dbg!(backoff.spin());
        }

        RwLockWriteGuard {
            ptr: self.data.get_mut(),
            state: &self.state,
        }
    }

    /// Attempts to acquire this `RwLock` for exclusive write access.
    ///
    /// If the access could not be granted at this time, this method returns
    /// [`None`]. Otherwise, [`Some`]`(`[`RwLockWriteGuard`]`)` containing a
    /// RAII guard is returned. The write access is released when it is dropped.
    ///
    /// This function does not spin.
    pub fn try_write(&self) -> Option<RwLockWriteGuard<'_, T>> {
        if test_dbg!(self
            .state
            .compare_exchange(UNLOCKED, WRITER, Acquire, Relaxed))
        .is_ok()
        {
            return Some(RwLockWriteGuard {
                ptr: self.data.get_mut(),
                state: &self.state,
            });
        }

        None
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// Since this call borrows the `RwLock` mutably, no actual locking needs to
    /// take place -- the mutable borrow statically guarantees no locks exist.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut lock = maitake_sync::spin::RwLock::new(0);
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

impl<T> RwLock<T> {
    /// Consumes this `RwLock`, returning the guarded data.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: fmt::Debug> fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("RwLock");
        s.field("state", &self.state.load(Relaxed));
        match self.try_read() {
            Some(read) => s.field("data", &read),
            None => s.field("data", &format_args!("<write locked>")),
        };
        s.finish()
    }
}

impl<T: Default> Default for RwLock<T> {
    /// Creates a new `RwLock<T>`, with the `Default` value for T.
    fn default() -> RwLock<T> {
        RwLock::new(Default::default())
    }
}

impl<T> From<T> for RwLock<T> {
    /// Creates a new instance of an `RwLock<T>` which is unlocked.
    /// This is equivalent to [`RwLock::new`].
    fn from(t: T) -> Self {
        RwLock::new(t)
    }
}

unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}

// === impl RwLockReadGuard ===

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
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

impl<T: ?Sized, R: ?Sized> AsRef<R> for RwLockReadGuard<'_, T>
where
    T: AsRef<R>,
{
    #[inline]
    fn as_ref(&self) -> &R {
        self.deref().as_ref()
    }
}

impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        let _val = test_dbg!(self.state.fetch_sub(READER, Release));
        debug_assert_eq!(
            _val & WRITER,
            0,
            "tried to drop a read guard while write locked, something is Very Wrong!"
        )
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

/// A [`RwLockReadGuard`] only allows immutable (`&T`) access to a `T`.
/// Therefore, it is [`Send`] and [`Sync`] as long as `T` is [`Sync`], because
/// it can be used to *share* references to a `T` across multiple threads
/// (requiring `T: Sync`), but it *cannot* be used to move ownership of a `T`
/// across thread boundaries, as the `T` cannot be taken out of the lock through
/// a `RwLockReadGuard`.
unsafe impl<T: ?Sized + Sync> Send for RwLockReadGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for RwLockReadGuard<'_, T> {}

// === impl RwLockWriteGuard ===

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
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

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            // Safety: we are holding the lock, so it is okay to dereference the
            // mut pointer.
            self.ptr.deref()
        }
    }
}

impl<T: ?Sized, R: ?Sized> AsRef<R> for RwLockWriteGuard<'_, T>
where
    T: AsRef<R>,
{
    #[inline]
    fn as_ref(&self) -> &R {
        self.deref().as_ref()
    }
}

impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        let _val = test_dbg!(self.state.swap(UNLOCKED, Release));
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

/// A [`RwLockWriteGuard`] is only [`Send`] if `T` is [`Send`] and [`Sync`],
/// because it can be used to *move* a `T` across thread boundaries, as it
/// allows mutable access to the `T` that can be used with
/// [`core::mem::replace`] or [`core::mem::swap`].
unsafe impl<T: ?Sized + Send + Sync> Send for RwLockWriteGuard<'_, T> {}

/// A [`RwLockWriteGuard`] is only [`Sync`] if `T` is [`Send`] and [`Sync`],
/// because it can be used to *move* a `T` across thread boundaries, as it
/// allows mutable access to the `T` that can be used with
/// [`core::mem::replace`] or [`core::mem::swap`].
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLockWriteGuard<'_, T> {}

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
