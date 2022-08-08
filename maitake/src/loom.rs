#[allow(unused_imports)]
pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    #![allow(dead_code)]

    #[cfg(feature = "alloc")]
    pub(crate) mod alloc {
        use super::sync::Arc;
        use core::{
            future::Future,
            pin::Pin,
            task::{Context, Poll},
        };
        pub(crate) use loom::alloc::*;
        #[pin_project::pin_project]
        pub(crate) struct TrackFuture<F> {
            #[pin]
            inner: F,
            track: Arc<()>,
        }

        impl<F: Future> Future for TrackFuture<F> {
            type Output = TrackFuture<F::Output>;
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();
                this.inner.poll(cx).map(|inner| TrackFuture {
                    inner,
                    track: this.track.clone(),
                })
            }
        }

        impl<F> TrackFuture<F> {
            /// Wrap a `Future` in a `TrackFuture` that participates in Loom's
            /// leak checking.
            #[track_caller]
            pub(crate) fn new(inner: F) -> Self {
                Self {
                    inner,
                    track: Arc::new(()),
                }
            }

            /// Stop tracking this future, and return the inner value.
            pub(crate) fn into_inner(self) -> F {
                self.inner
            }
        }

        #[track_caller]
        pub(crate) fn track_future<F: Future>(inner: F) -> TrackFuture<F> {
            TrackFuture::new(inner)
        }
    }

    pub(crate) use loom::{cell, future, model, thread};

    pub(crate) mod sync {
        pub(crate) use loom::sync::*;

        pub(crate) mod spin {
            pub(crate) use loom::sync::MutexGuard;

            /// Mock version of mycelium's spinlock, but using
            /// `loom::sync::Mutex`. The API is slightly different, since the
            /// mycelium mutex does not support poisoning.
            #[derive(Debug)]
            pub(crate) struct Mutex<T>(loom::sync::Mutex<T>);

            impl<T> Mutex<T> {
                #[track_caller]
                pub(crate) fn new(t: T) -> Self {
                    Self(loom::sync::Mutex::new(t))
                }

                #[track_caller]
                pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
                    self.0.try_lock().ok()
                }

                #[track_caller]
                pub fn lock(&self) -> MutexGuard<'_, T> {
                    self.0.lock().expect("loom mutex will never poison")
                }
            }
        }
    }
}

#[cfg(not(loom))]
mod inner {
    #![allow(dead_code)]
    pub(crate) mod sync {
        #[cfg(feature = "alloc")]
        pub use alloc::sync::*;
        pub use core::sync::*;

        pub use mycelium_util::sync::spin;
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
        pub(crate) struct UnsafeCell<T: ?Sized>(core::cell::UnsafeCell<T>);

        impl<T> UnsafeCell<T> {
            pub const fn new(data: T) -> UnsafeCell<T> {
                UnsafeCell(core::cell::UnsafeCell::new(data))
            }
        }

        impl<T: ?Sized> UnsafeCell<T> {
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
