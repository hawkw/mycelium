use core::{any, cell::UnsafeCell, fmt, mem::MaybeUninit, ops::Deref};

use crate::sync::atomic::{AtomicU8, Ordering};

/// A cell which represents a promise to initialize some piece of data once,
/// before it will be accessed.
///
/// In debug mode, accesses to this cell will check whether or not it has been
/// initialized. In release mode, **these checks are elided**. This means that
/// if you dereference a `SingleInit<T>` in release mode without having first
/// initialized it, YOU WILL READ UNINITIALIZED MEMORY. However, when the data
/// is accessed frequently enough for the performance penalty of a single atomic
/// load to matter, this may be worth it.
// TODO(eliza): maybe this whole thing is just an incredibly bad idea...
pub struct SingleInit<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

impl<T> SingleInit<T> {
    const UNINITIALIZED: u8 = 0;
    const INITIALIZING: u8 = 1;
    const INITIALIZED: u8 = 2;

    /// # Safety
    ///
    /// Callers must ensure they **DON'T FUCK IT UP**.
    pub const unsafe fn uninitialized() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(Self::UNINITIALIZED),
        }
    }

    #[track_caller]
    pub fn initialize(&self, value: T) {
        if let Err(actual) = self.state.compare_exchange(
            Self::UNINITIALIZED,
            Self::INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            panic!(
                "SingleInit<{}>: attempted to initialize twice! (state={})",
                any::type_name::<T>(),
                actual
            );
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
                "SingleInit<{}>: state changed while locked. This is a bug! (state={})",
                any::type_name::<T>(),
                actual
            );
        }
    }
}

impl<T> core::ops::Deref for SingleInit<T> {
    type Target = T;

    #[cfg_attr(not(debug_assertions), inline(always))]
    #[cfg_attr(debug_assertions, track_caller)]
    fn deref(&self) -> &Self::Target {
        debug_assert_eq!(
            Self::INITIALIZED,
            self.state.load(Ordering::Acquire),
            "SingleInit<{}>: accessed before initialized!\n\
            /!\\ EXTREMELY SERIOUS WARNING: /!\\ This is REAL BAD! If you were \
            running in release mode, you would have just read uninitialized \
            memory! That's bad news indeed, buddy. Double- or triple-check \
            your assumptions, or consider Just Using A Goddamn Mutex --- it's \
            much safer that way. Maybe this whole `SingleInit` thing was a \
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

impl<T: fmt::Debug> fmt::Debug for SingleInit<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f
            .debug_struct("SingleInit")
            .field("type", &any::type_name::<T>());
        match self.state.load(Ordering::Acquire) {
            Self::INITIALIZED => d.field("value", Deref::deref(self)).finish(),
            _ => d.field("value", &format_args!("<uninitialized>")).finish(),
        }
    }
}
