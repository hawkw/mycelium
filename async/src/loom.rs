pub(crate) use self::inner::*;

#[cfg(test)]
mod inner {
    pub(crate) mod atomic {
        pub use loom::sync::atomic::*;
        pub use std::sync::atomic::Ordering;
    }
    pub(crate) use loom::{cell::UnsafeCell, future, hint, sync, thread};

    pub(crate) fn model(f: impl Fn() + Sync + Send + 'static) {
        let iteration = core::sync::atomic::AtomicUsize::new(0);
        loom::model(move || {
            println!(
                "\n---- iteration {} ----\n",
                iteration.fetch_add(1, atomic::Ordering::Relaxed)
            );
            f();
        })
    }

    pub(crate) mod alloc {
        #![allow(dead_code)]
        use loom::alloc;
        use std::fmt;
        /// Track allocations, detecting leaks
        ///
        /// This is a version of `loom::alloc::Track` that adds a missing
        /// `Default` impl.
        pub struct Track<T>(alloc::Track<T>);

        impl<T> Track<T> {
            /// Track a value for leaks
            #[inline(always)]
            pub fn new(value: T) -> Track<T> {
                Track(alloc::Track::new(value))
            }

            /// Get a reference to the value
            #[inline(always)]
            pub fn get_ref(&self) -> &T {
                self.0.get_ref()
            }

            /// Get a mutable reference to the value
            #[inline(always)]
            pub fn get_mut(&mut self) -> &mut T {
                self.0.get_mut()
            }

            /// Stop tracking the value for leaks
            #[inline(always)]
            pub fn into_inner(self) -> T {
                self.0.into_inner()
            }
        }

        impl<T: fmt::Debug> fmt::Debug for Track<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl<T: Default> Default for Track<T> {
            fn default() -> Self {
                Self::new(T::default())
            }
        }
    }
}

#[cfg(not(test))]
mod inner {
    #![allow(dead_code)]
    pub(crate) mod sync {
        pub use core::sync::*;

        #[cfg(feature = "alloc")]
        pub use alloc::sync::*;
    }

    pub(crate) use core::sync::atomic;

    #[cfg(feature = "std")]
    pub use std::thread;

    pub(crate) mod hint {
        #[inline(always)]
        pub(crate) fn spin_loop() {
            // MSRV: std::hint::spin_loop() stabilized in 1.49.0
            #[allow(deprecated)]
            super::atomic::spin_loop_hint()
        }
    }

    #[derive(Debug)]
    pub(crate) struct UnsafeCell<T>(core::cell::UnsafeCell<T>);

    impl<T> UnsafeCell<T> {
        pub const fn new(data: T) -> UnsafeCell<T> {
            UnsafeCell(core::cell::UnsafeCell::new(data))
        }

        #[inline(always)]
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.0.get())
        }

        #[inline(always)]
        pub fn with_mut<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.0.get())
        }
    }

    pub(crate) mod alloc {
        /// Track allocations, detecting leaks
        #[derive(Debug, Default)]
        pub struct Track<T> {
            value: T,
        }

        impl<T> Track<T> {
            /// Track a value for leaks
            #[inline(always)]
            pub fn new(value: T) -> Track<T> {
                Track { value }
            }

            /// Get a reference to the value
            #[inline(always)]
            pub fn get_ref(&self) -> &T {
                &self.value
            }

            /// Get a mutable reference to the value
            #[inline(always)]
            pub fn get_mut(&mut self) -> &mut T {
                &mut self.value
            }

            /// Stop tracking the value for leaks
            #[inline(always)]
            pub fn into_inner(self) -> T {
                self.value
            }
        }
    }
}
