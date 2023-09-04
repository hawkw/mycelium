//! Cells storing a value which must be initialized prior to use.
//!
//! This module provides:
//!
//! - [`InitOnce`]: a cell storing a [`MaybeUninit`](core::mem::MaybeUninit)
//!       value which must be manually initialized prior to use.
//! - [`Lazy`]: an [`InitOnce`] cell coupled with an initializer function. The
//!       [`Lazy`] cell ensures the initializer is called to initialize the
//!       value the first time it is accessed.
use crate::{
    loom::sync::atomic::{AtomicU8, Ordering},
    util::{Backoff, CheckedMaybeUninit},
};
use core::{
    any,
    cell::UnsafeCell,
    fmt,
    ops::{Deref, DerefMut},
};

/// A cell which may be initialized a single time after it is created.
///
/// This can be used as a safer alternative to `static mut`.
///
/// For performance-critical use-cases, this type also has a [`get_unchecked`]
/// method, which dereferences the cell **without** checking if it has been
/// initialized. This method is unsafe and should be used with caution ---
/// incorrect usage can result in reading uninitialized memory.
///
/// [`get_unchecked`]: Self::get_unchecked
pub struct InitOnce<T> {
    value: UnsafeCell<CheckedMaybeUninit<T>>,
    state: AtomicU8,
}

/// A cell which will be lazily initialized by the provided function the first
/// time it is accessed.
///
/// This can be used as a safer alternative to `static mut`.
pub struct Lazy<T, F = fn() -> T> {
    value: UnsafeCell<CheckedMaybeUninit<T>>,
    state: AtomicU8,
    initializer: F,
}

/// Errors returned by [`InitOnce::try_init`].
///
/// This contains the value that the caller was attempting to use to initialize
/// the cell.
pub struct TryInitError<T> {
    value: T,
    actual: u8,
}

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const INITIALIZED: u8 = 2;

// === impl InitOnce ===

impl<T> InitOnce<T> {
    loom_const_fn! {
        /// Returns a new `InitOnce` in the uninitialized state.
        #[must_use]
        pub fn uninitialized() -> Self {
            Self {
                value: UnsafeCell::new(CheckedMaybeUninit::uninit()),
                state: AtomicU8::new(UNINITIALIZED),
            }
        }
    }

    /// Initialize the cell to `value`, returning an error if it has already
    /// been initialized.
    ///
    /// If the cell has already been initialized, the returned error contains
    /// the value.
    pub fn try_init(&self, value: T) -> Result<(), TryInitError<T>> {
        if let Err(actual) = self.state.compare_exchange(
            UNINITIALIZED,
            INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            return Err(TryInitError { value, actual });
        };
        unsafe {
            *(self.value.get()) = CheckedMaybeUninit::new(value);
        }
        let _prev = self.state.swap(INITIALIZED, Ordering::AcqRel);
        debug_assert_eq!(
            _prev,
            INITIALIZING,
            "InitOnce<{}>: state changed while locked. This is a bug!",
            any::type_name::<T>(),
        );
        Ok(())
    }

    /// Initialize the cell to `value`, panicking if it has already been
    /// initialized.
    ///
    /// # Panics
    ///
    /// If the cell has already been initialized.
    #[track_caller]
    pub fn init(&self, value: T) -> &T {
        self.try_init(value).unwrap();
        self.get()
    }

    /// Borrow the contents of this `InitOnce` cell, if it has been
    /// initialized. Otherwise, if the cell has not yet been initialized, this
    /// returns [`None`].
    #[inline]
    #[must_use]
    pub fn try_get(&self) -> Option<&T> {
        if self.state.load(Ordering::Acquire) != INITIALIZED {
            return None;
        }
        unsafe {
            // Safety: we just checked if the value was initialized.
            Some(&*((*self.value.get()).as_ptr()))
        }
    }

    /// Borrow the contents of this `InitOnce` cell, or panic if it has not
    /// been initialized.
    ///
    /// # Panics
    ///
    /// If the cell has not yet been initialized.
    #[track_caller]
    #[inline]
    #[must_use]
    pub fn get(&self) -> &T {
        if self.state.load(Ordering::Acquire) != INITIALIZED {
            panic!("InitOnce<{}> not yet initialized!", any::type_name::<T>());
        }
        unsafe {
            // Safety: we just checked if the value was initialized.
            &*((*self.value.get()).as_ptr())
        }
    }

    /// Borrow the contents of this `InitOnce` cell, or initialize it with the
    /// provided closure.
    ///
    /// If the cell has been initialized, this returns the current value.
    /// Otherwise, it calls the closure, puts the returned value from the
    /// closure in the cell, and borrows the current value.
    #[must_use]
    pub fn get_or_else(&self, f: impl FnOnce() -> T) -> &T {
        if let Some(val) = self.try_get() {
            return val;
        }

        let _ = self.try_init(f());
        self.get()
    }

    /// Borrow the contents of this `InitOnce` cell, **without** checking
    /// whether it has been initialized.
    ///
    /// # Safety
    ///
    /// The caller is responsible for ensuring that the value has already been
    /// initialized.
    ///
    /// In debug mode, this still checks the state of the cell, so if it has yet
    /// to be initialized, this will panic. However, in release mode builds,
    /// this is completely unchecked. If the value has not yet been initialized,
    /// this may return a pointer to uninitialized memory! It may also return a
    /// pointer to memory that is currently being written to.
    ///
    /// If you see this method panic in debug mode, please, **please** re-check
    /// your code.
    #[cfg_attr(not(debug_assertions), inline(always))]
    #[cfg_attr(debug_assertions, track_caller)]
    #[must_use]
    pub unsafe fn get_unchecked(&self) -> &T {
        debug_assert_eq!(
            INITIALIZED,
            self.state.load(Ordering::Acquire),
            "InitOnce<{}>: accessed before initialized!\n\
            /!\\ EXTREMELY SERIOUS WARNING: /!\\ This is REAL BAD! If you were \
            running in release mode, you would have just read uninitialized \
            memory! That's bad news indeed, buddy. Double- or triple-check \
            your assumptions, or consider Just Using A Goddamn Mutex --- it's \
            much safer that way. Maybe this whole `InitOnce` thing was a \
            mistake...
            ",
            any::type_name::<T>(),
        );
        unsafe {
            // Safety: hahaha wheeee no rules! You can't stop meeeeee!
            &*((*self.value.get()).as_ptr())
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for InitOnce<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.state.load(Ordering::Acquire) {
            INITIALIZED => self.get().fmt(f),
            INITIALIZING => f.pad("<initializing>"),
            UNINITIALIZED => f.pad("<uninitialized>"),
            _state => unsafe {
                unreachable_unchecked!("unexpected state value {}, this is a bug!", _state)
            },
        }
    }
}

unsafe impl<T: Send> Send for InitOnce<T> {}
unsafe impl<T: Sync> Sync for InitOnce<T> {}

// === impl Lazy ===

impl<T, F> Lazy<T, F> {
    loom_const_fn! {
        /// Returns a new `Lazy` cell, initialized with the provided `initializer`
        /// function.
        #[must_use]
        pub fn new(initializer: F) -> Self {
            Self {
                value: UnsafeCell::new(CheckedMaybeUninit::uninit()),
                state: AtomicU8::new(UNINITIALIZED),
                initializer,
            }
        }
    }

    /// Returns the value of the lazy cell, if it has already been initialized.
    /// Otherwise, returns `None`.
    #[inline]
    #[must_use]
    pub fn get_if_present(&self) -> Option<&T> {
        if self.state.load(Ordering::Acquire) == INITIALIZED {
            let value = unsafe {
                // Safety: we just ensured the cell was initialized.
                &*((*self.value.get()).as_ptr())
            };
            Some(value)
        } else {
            None
        }
    }
}

impl<T, F> Lazy<T, F>
where
    F: Fn() -> T,
{
    /// Borrow the value, or initialize it if it has not yet been initialized.
    #[inline]
    #[must_use]
    pub fn get(&self) -> &T {
        self.init();
        unsafe {
            // Safety: we just ensured the cell was initialized.
            &*((*self.value.get()).as_ptr())
        }
    }

    /// Borrow the value mutably, or initialize it if it has not yet been initialized.
    #[inline]
    #[must_use]
    pub fn get_mut(&mut self) -> &mut T {
        self.init();
        unsafe {
            // Safety: we just ensured the cell was initialized.
            &mut *((*self.value.get()).as_mut_ptr())
        }
    }

    /// Ensure that the cell has been initialized.
    ///
    /// If the cell has yet to be initialized, this initializes it. If it is
    /// currently initializing, this spins until it has been fully initialized.
    /// Otherwise, this returns immediately.
    pub fn init(&self) {
        let state = self.state.compare_exchange(
            UNINITIALIZED,
            INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );

        match state {
            Err(INITIALIZED) => {
                // Already initialized! Just return the value.
            }
            Err(INITIALIZING) => {
                // Wait for the cell to be initialized
                let mut backoff = Backoff::new();
                while self.state.load(Ordering::Acquire) != INITIALIZED {
                    backoff.spin();
                }
            }
            Ok(_) => {
                // Now we have to actually initialize the cell.
                unsafe {
                    *(self.value.get()) = CheckedMaybeUninit::new((self.initializer)());
                }
                if let Err(actual) = self.state.compare_exchange(
                    INITIALIZING,
                    INITIALIZED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    unreachable!(
                        "Lazy<{}>: state changed while locked. This is a bug! (state={})",
                        any::type_name::<T>(),
                        actual
                    );
                }
            }
            Err(_state) => unsafe {
                unreachable_unchecked!(
                    "Lazy<{}>: unexpected state {}!. This is a bug!",
                    any::type_name::<T>(),
                    _state
                )
            },
        };
    }
}

impl<T, F> Deref for Lazy<T, F>
where
    F: Fn() -> T,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T, F> DerefMut for Lazy<T, F>
where
    F: Fn() -> T,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

impl<T, F> fmt::Debug for Lazy<T, F>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.state.load(Ordering::Acquire) {
            INITIALIZED => self
                .get_if_present()
                .expect("if state is `INITIALIZED`, value should be present")
                .fmt(f),
            INITIALIZING => f.pad("<initializing>"),
            UNINITIALIZED => f.pad("<uninitialized>"),
            _state => unsafe {
                unreachable_unchecked!("unexpected state value {}, this is a bug!", _state)
            },
        }
    }
}

unsafe impl<T: Send, F: Send> Send for Lazy<T, F> {}
unsafe impl<T: Sync, F: Sync> Sync for Lazy<T, F> {}

// === impl TryInitError ===

impl<T> TryInitError<T> {
    /// Returns the value that the caller attempted to initialize the cell with
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> fmt::Debug for TryInitError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TryInitError")
            .field("type", &any::type_name::<T>())
            .field("value", &format_args!("..."))
            .field(
                "state",
                &format_args!("State::{}", match self.actual {
                    UNINITIALIZED => "UNINITIALIZED",
                    INITIALIZING => "INITIALIZING",
                    INITIALIZED => unsafe { unreachable_unchecked!("an error should not be returned when InitOnce is in the initialized state, this is a bug!") },
                    _state => unsafe { unreachable_unchecked!("unexpected state value {}, this is a bug!", _state) },
                }),
            )
            .finish()
    }
}

impl<T> fmt::Display for TryInitError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InitOnce<{}> already initialized", any::type_name::<T>())
    }
}

#[cfg(feature = "core-error")]
impl<T> core::error::Error for TryInitError<T> {}
