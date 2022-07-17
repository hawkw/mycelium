//! A variant of [`core::cell::UnsafeCell`] specialized for use in
//! implementations of synchronization primitives.
//!
//! When the `cfg(loom)` flag is enabled, the [`UnsafeCell`] type in this module
//! is a re-export of [`loom`]'s [checked `UnsafeCell` type][loom-unsafecell].
//!
//! [`loom`]: crates.io/crates/loom
//! [loom-unsafecell]: https://docs.rs/loom/latest/loom/cell/struct.UnsafeCell.html

pub use self::unsafe_cell::*;

#[cfg(loom)]
mod unsafe_cell {
    pub use loom::cell::{ConstPtr, MutPtr, UnsafeCell};
}

#[cfg(not(loom))]
mod unsafe_cell {
    #![allow(dead_code)]
    use core::cell;

    /// `UnsafeCell` ensures access to the inner value are valid under the Rust memory
    /// model.
    ///
    /// When not running under `loom`, this is identical to an `UnsafeCell`.
    #[derive(Debug)]
    pub struct UnsafeCell<T> {
        data: cell::UnsafeCell<T>,
    }

    #[derive(Debug)]
    pub(crate) struct ConstPtr<T: ?Sized>(*const T);

    #[derive(Debug)]
    pub(crate) struct MutPtr<T: ?Sized>(*mut T);

    impl<T> UnsafeCell<T> {
        /// Construct a new instance of `CausalCell` which will wrap the specified
        /// value.
        pub const fn new(data: T) -> Self {
            Self {
                data: cell::UnsafeCell::new(data),
            }
        }

        /// Get an immutable pointer to the wrapped value.
        ///
        /// # Panics
        ///
        /// When running under `loom`, this function will panic if the access is
        /// not valid under the Rust memory model.
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.data.get() as *const _)
        }

        /// Get a mutable pointer to the wrapped value.
        ///
        /// # Panics
        ///
        /// When running under `loom`, this function will panic if the access is
        /// not valid under the Rust memory model.
        pub fn with_mut<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.data.get())
        }

        /// Get an immutable pointer to the wrapped value.
        pub fn with_unchecked<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.data.get())
        }

        /// Get a mutable pointer to the wrapped value.
        pub fn with_mut_unchecked<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.data.get())
        }

        #[inline(always)]
        pub(crate) fn get(&self) -> ConstPtr<T> {
            ConstPtr(self.data.get())
        }

        #[inline(always)]
        pub(crate) fn get_mut(&self) -> MutPtr<T> {
            MutPtr(self.data.get())
        }
    }

    // === impl MutPtr ===

    impl<T: ?Sized> MutPtr<T> {
        // Clippy knows that it's Bad and Wrong to construct a mutable reference
        // from an immutable one...but this function is intended to simulate a raw
        // pointer, so we have to do that here.
        #[allow(clippy::mut_from_ref)]
        #[inline(always)]
        pub(crate) unsafe fn deref(&self) -> &mut T {
            &mut *self.0
        }

        #[inline(always)]
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.0)
        }
    }

    // === impl ConstPtr ===

    impl<T: ?Sized> ConstPtr<T> {
        #[inline(always)]
        pub(crate) unsafe fn deref(&self) -> &T {
            &*self.0
        }

        #[inline(always)]
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.0)
        }
    }
}
