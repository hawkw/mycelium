use core::{any, cell::UnsafeCell, fmt, mem::MaybeUninit, ops::Deref};

use crate::sync::atomic::{AtomicU8, Ordering};

/// A cell which which may be initialized a single time after it is created.
///
/// This can be used as a safer alternative to `static mut`.
pub struct InitOnce<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

pub struct TryInitError<T> {
    value: T,
}

impl<T> InitOnce<T> {
    const UNINITIALIZED: u8 = 0;
    const INITIALIZING: u8 = 1;
    const INITIALIZED: u8 = 2;

    pub const fn uninitialized() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(Self::UNINITIALIZED),
        }
    }

    /// Initialize the cell to `value`, returning an error if it has already
    /// been initialized.
    ///
    /// If the cell has already been initialized, the returned error contains
    /// the value.
    pub fn try_init(&self, value: T) -> Result<(), TryInitError<T>> {
        if let Err(actual) = self.state.compare_exchange(
            Self::UNINITIALIZED,
            Self::INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            return Err(TryInitError { value });
        };
        unsafe {
            *(self.value.get()) = MaybeUninit::new(value);
        }
        if let Err(actual) = self.state.compare_exchange(
            Self::INITIALIZING,
            Self::INITIALIZED,
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
        if self.state.load(Ordering::Acquire) != Self::INITIALIZED {
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
            Self::INITIALIZED,
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
        let mut d = f
            .debug_struct("InitOnce")
            .field("type", &any::type_name::<T>());
        match self.state.load(Ordering::Acquire) {
            Self::INITIALIZED => d.field("value", Deref::deref(self)).finish(),
            _ => d.field("value", &format_args!("<uninitialized>")).finish(),
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
            .finish()
    }
}

impl<T> fmt::Display for TryInitError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InitOnce<{}> already initialized", any::type_name::<T>())
    }
}

impl<T> crate::error::Error for TryInitError<T> {}
