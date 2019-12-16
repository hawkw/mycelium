pub use self::causal::*;
#[cfg(any(test, feature = "loom"))]
mod causal {
    pub use loom::cell::{CausalCell, CausalCheck};
}

#[cfg(all(not(test), not(feature = "loom")))]
mod causal {
    use core::cell::UnsafeCell;

    /// CausalCell ensures access to the inner value are valid under the Rust memory
    /// model.
    ///
    /// When not running under `loom`, this is identical to an `UnsafeCell`.
    #[derive(Debug)]
    pub struct CausalCell<T> {
        data: UnsafeCell<T>,
    }

    unsafe impl<T> Sync for CausalCell<T> {}

    /// Deferred causal cell check.
    ///
    /// When not running under `loom`, this does nothing.
    #[derive(Debug)]
    #[must_use]
    pub struct CausalCheck {
        _p: (),
    }

    impl<T> CausalCell<T> {
        /// Construct a new instance of `CausalCell` which will wrap the specified
        /// value.
        pub const fn new(data: T) -> CausalCell<T> {
            Self {
                data: UnsafeCell::new(data),
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

        /// Get an immutable pointer to the wrapped value, deferring the causality
        /// check.
        ///
        /// # Panics
        ///
        /// When running under `loom`, this function will panic if the access is
        /// not valid under the Rust memory model.
        pub fn with_deferred<F, R>(&self, f: F) -> (R, CausalCheck)
        where
            F: FnOnce(*const T) -> R,
        {
            (f(self.data.get() as *const _), CausalCheck::default())
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

        /// Get a mutable pointer to the wrapped value.
        ///
        /// # Panics
        ///
        /// When running under `loom`, this function will panic if the access is
        /// not valid under the Rust memory model.
        pub fn with_deferred_mut<F, R>(&self, f: F) -> (R, CausalCheck)
        where
            F: FnOnce(*mut T) -> R,
        {
            (f(self.data.get()), CausalCheck::default())
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

        /// Check that the current thread can make an immutable access without
        /// violating the Rust memory model.
        ///
        /// Specifically, this function checks that there is no concurrent mutable
        /// access with this immutable access, while allowing many concurrent
        /// immutable accesses.
        ///
        /// When not running under `loom`, this does nothing.
        pub fn check(&self) {}

        /// Check that the current thread can make a mutable access without violating
        /// the Rust memory model.
        ///
        /// Specifically, this function checks that there is no concurrent mutable
        /// access and no concurrent immutable access(es) with this mutable
        /// access.
        ///
        /// When not running under `loom`, this does nothing.
        pub fn check_mut(&self) {}
    }

    impl CausalCheck {
        /// Panic if the CausaalCell access was invalid.
        ///
        /// When not running under `loom`, this does nothing.
        pub fn check(mut self) {
            self = CausalCheck::default();
        }

        /// Merge this check with another check
        ///
        /// When not running under `loom`, this does nothing.
        pub fn join(&mut self, _: CausalCheck) {}
    }

    impl Default for CausalCheck {
        fn default() -> CausalCheck {
            CausalCheck { _p: () }
        }
    }
}
