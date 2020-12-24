use core::{
    any,
    cell::UnsafeCell,
    fmt,
    mem::MaybeUninit,
    ops::Deref,
    sync::atomic::{AtomicU8, Ordering},
};

/// A cell which may be initialized a single time after it is created.
///
/// This can be used as a safer alternative to `static mut`.
///
/// For performance-critical use-cases, this type also has a [`get_unchecked`]
/// method, which dereferences the cell **without** checking if it has been
/// initialized. This method is unsafe and should be used with caution ---
/// incorrect usage can result in reading uninitialized memory.
pub struct InitOnce<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

/// A cell which will be lazily initialized by the provided function the first
/// time it is accessed.
///
/// This can be used as a safer alternative to `static mut`.
///
/// For performance-critical use-cases, this type also has a [`get_unchecked`]
/// method, which dereferences the cell **without** checking if it has been
/// initialized. This method is unsafe and should be used with caution ---
/// incorrect usage can result in reading uninitialized memory.
pub struct Lazy<T, F = fn() -> T> {
    value: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
    initializer: F,
}

pub struct TryInitError<T> {
    value: T,
    actual: u8,
}

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const INITIALIZED: u8 = 2;

// === impl InitOnce ===

impl<T> InitOnce<T> {
    pub const fn uninitialized() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(UNINITIALIZED),
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
            *(self.value.get()) = MaybeUninit::new(value);
        }
        if let Err(actual) = self.state.compare_exchange(
            INITIALIZING,
            INITIALIZED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            unreachable!(
                "InitOnce<{}>: state changed while locked. This is a bug! (state={})",
                any::type_name::<T>(),
                actual
            );
        }
        Ok(())
    }

    /// Initialize the cell to `value`, panicking if it has already been
    /// initialized.
    #[track_caller]
    pub fn init(&self, value: T) {
        self.try_init(value).unwrap()
    }

    /// Borrow the contents of this `InitOnce` cell, if it has been
    /// initialized. Otherwise, if the cell has not yet been initialized, this
    /// returns `None`.
    #[inline]
    pub fn get(&self) -> Option<&T> {
        if self.state.load(Ordering::Acquire) != INITIALIZED {
            return None;
        }
        unsafe {
            // Safety: we just checked if the value was initialized.
            Some(&*((*self.value.get()).as_ptr()))
        }
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

impl<T> core::ops::Deref for InitOnce<T> {
    type Target = T;

    #[track_caller]
    #[inline]
    fn deref(&self) -> &Self::Target {
        match self.get() {
            Some(t) => t,
            None => panic!("InitOnce<{}> not yet initialized!", any::type_name::<T>(),),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for InitOnce<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("InitOnce");
        d.field("type", &any::type_name::<T>());
        match self.state.load(Ordering::Acquire) {
            INITIALIZED => d.field("value", Deref::deref(self)).finish(),
            INITIALIZING => d.field("value", &format_args!("<initializing>")).finish(),
            UNINITIALIZED => d.field("value", &format_args!("<uninitialized>")).finish(),
            state => unreachable!("unexpected state value {}, this is a bug!", state),
        }
    }
}

// === impl Lazy ===

impl<T, F> Lazy<T, F> {
    pub const fn new(initializer: F) -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(UNINITIALIZED),
            initializer,
        }
    }
}

impl<T, F> Lazy<T, F>
where
    F: Fn() -> T,
{
    /// Initialize the cell to `value`, returning an error if it has already
    /// been initialized.
    ///
    /// If the cell has already been initialized, the returned error contains
    /// the value.
    pub fn get(&self) -> &T {
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
                let mut backoff = super::Backoff::new();
                while self.state.load(Ordering::Acquire) != INITIALIZED {
                    backoff.spin();
                }
            }
            Ok(_) => {
                // Now we have to actually initialize the cell.
                unsafe {
                    *(self.value.get()) = MaybeUninit::new((self.initializer)());
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
            Err(state) => unreachable!(
                "Lazy<{}>: unexpected state {}!. This is a bug!",
                any::type_name::<T>(),
                state
            ),
        };
        unsafe {
            // Safety: we just ensured the cell was initialized.
            &*((*self.value.get()).as_ptr())
        }
    }
}

impl<T, F> Deref for Lazy<T, F>
where
    F: Fn() -> T,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T: fmt::Debug, F> fmt::Debug for Lazy<T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Lazy");
        d.field("type", &any::type_name::<T>())
            .field("initializer", &format_args!("..."));
        match self.state.load(Ordering::Acquire) {
            INITIALIZED => d.field("value", Deref::deref(self)).finish(),
            INITIALIZING => d.field("value", &format_args!("<initializing>")).finish(),
            UNINITIALIZED => d.field("value", &format_args!("<uninitialized>")).finish(),
            state => unreachable!("unexpected state value {}, this is a bug!", state),
        }
    }
}

// === impl TryInitError ===

impl<T> TryInitError<T> {
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
                    INITIALIZED => unreachable!("an error should not be returned when InitOnce is in the initialized state, this is a bug!"),
                    state => unreachable!("unexpected state value {}, this is a bug!", state),
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

impl<T> crate::error::Error for TryInitError<T> {}
