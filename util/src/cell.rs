pub use self::causal::*;
#[cfg(test)]
mod causal {
    pub use loom::cell::UnsafeCell as CausalCell;
}

#[cfg(not(test))]
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
    }
}
