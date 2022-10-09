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
    pub(crate) mod thread {
        #[allow(unused_imports)]
        pub(crate) use std::thread::{JoinHandle, Thread};
        pub fn spawn<F, T>(f: F) -> JoinHandle<T>
        where
            F: FnOnce() -> T,
            F: Send + 'static,
            T: Send + 'static,
        {
            let dispatch = tracing::dispatch::Dispatch::default();
            std::thread::spawn(move || {
                let _guard = tracing::dispatch::set_default(&dispatch);
                test_info!("thread spawned");
                f()
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn model(f: impl Fn()) {
        let _trace = crate::test_util::trace_init();
        f()
    }

    #[cfg(test)]
    pub(crate) mod model {
        #[non_exhaustive]
        #[derive(Default)]
        pub(crate) struct Builder {
            pub(crate) max_threads: usize,
            pub(crate) max_branches: usize,
            pub(crate) max_permutations: Option<usize>,
            // pub(crate) max_duration: Option<Duration>,
            pub(crate) preemption_bound: Option<usize>,
            // pub(crate) checkpoint_file: Option<PathBuf>,
            pub(crate) checkpoint_interval: usize,
            pub(crate) location: bool,
            pub(crate) log: bool,
        }

        impl Builder {
            pub(crate) fn new() -> Self {
                Self::default()
            }

            pub(crate) fn check(&self, f: impl Fn()) {
                super::model(f)
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
}
