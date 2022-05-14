#[allow(unused_imports)]
pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    pub use loom::{alloc, cell, hint, model, sync, thread};
}

#[cfg(not(loom))]
mod inner {
    #![allow(dead_code)]
    pub(crate) mod sync {
        pub use alloc::sync::*;
        pub use core::sync::*;
    }

    pub(crate) use core::sync::atomic;

    #[cfg(test)]
    pub use std::thread;

    pub(crate) mod hint {
        #[inline(always)]
        pub(crate) fn spin_loop() {
            // MSRV: std::hint::spin_loop() stabilized in 1.49.0
            #[allow(deprecated)]
            super::atomic::spin_loop_hint()
        }
    }

    pub(crate) mod cell {
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

            #[inline(always)]
            pub(crate) fn get_mut(&self) -> MutPtr<T> {
                MutPtr(self.0.get())
            }
        }

        #[derive(Debug)]
        pub(crate) struct MutPtr<T: ?Sized>(*mut T);

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

    #[cfg(test)]
    pub(crate) fn traceln(args: std::fmt::Arguments) {
        eprintln!("{}", args);
    }

    #[cfg(not(test))]
    pub(crate) fn traceln(_: core::fmt::Arguments) {}
}
