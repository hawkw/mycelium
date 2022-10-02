#[allow(unused_imports)]
pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    pub use loom::{alloc, hint, model, sync, thread};
}

#[cfg(not(loom))]
mod inner {
    #![allow(dead_code)]

    #[cfg(test)]
    pub(crate) use std::thread;

    #[cfg(test)]
    pub(crate) fn model(f: impl Fn()) {
        f()
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
