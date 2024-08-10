#[allow(unused_imports)]
pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    #![allow(dead_code)]
    #![allow(unused_imports)]

    #[cfg(feature = "alloc")]
    pub(crate) mod alloc {
        use super::sync::Arc;
        use core::{
            future::Future,
            pin::Pin,
            task::{Context, Poll},
        };
        pub(crate) use loom::alloc::*;

        #[derive(Debug)]
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

        // PartialEq impl so that `assert_eq!(..., Ok(...))` works
        impl<F: PartialEq> PartialEq for TrackFuture<F> {
            fn eq(&self, other: &Self) -> bool {
                self.inner == other.inner
            }
        }
    }

    #[cfg(test)]
    pub(crate) use loom::future;
    pub(crate) use loom::{cell, hint, model, thread};

    pub(crate) mod sync {
        pub(crate) use loom::sync::*;

        pub(crate) mod blocking {
            use core::{
                marker::PhantomData,
                ops::{Deref, DerefMut},
            };

            #[cfg(feature = "tracing")]
            use core::panic::Location;

            use core::fmt;

            /// Mock version of mycelium's spinlock, but using
            /// `loom::sync::Mutex`. The API is slightly different, since the
            /// mycelium mutex does not support poisoning.
            pub(crate) struct Mutex<T, Lock = crate::spin::Spinlock>(
                loom::sync::Mutex<T>,
                PhantomData<Lock>,
            );

            pub(crate) struct MutexGuard<'a, T, Lock = crate::spin::Spinlock> {
                guard: loom::sync::MutexGuard<'a, T>,
                #[cfg(feature = "tracing")]
                location: &'static Location<'static>,
                _p: PhantomData<Lock>,
            }

            impl<T, Lock> Mutex<T, Lock> {
                #[track_caller]
                pub(crate) fn new(t: T) -> Self {
                    Self(loom::sync::Mutex::new(t), PhantomData)
                }

                #[track_caller]
                pub(crate) fn new_with_raw_mutex(t: T, _: Lock) -> Self {
                    Self::new(t)
                }

                #[track_caller]
                pub fn with_lock<U>(&self, f: impl FnOnce(&mut T) -> U) -> U {
                    let mut guard = self.lock();
                    let res = f(&mut *guard);
                    res
                }

                #[track_caller]
                pub fn try_lock(&self) -> Option<MutexGuard<'_, T, Lock>> {
                    #[cfg(feature = "tracing")]
                    let location = Location::caller();
                    #[cfg(feature = "tracing")]
                    tracing::debug!(%location, "Mutex::try_lock");

                    match self.0.try_lock() {
                        Ok(guard) => {
                            #[cfg(feature = "tracing")]
                            tracing::debug!(%location, "Mutex::try_lock -> locked!");
                            Some(MutexGuard {
                                guard,

                                #[cfg(feature = "tracing")]
                                location,
                                _p: PhantomData,
                            })
                        }
                        Err(_) => {
                            #[cfg(feature = "tracing")]
                            tracing::debug!(%location, "Mutex::try_lock -> already locked");
                            None
                        }
                    }
                }

                #[track_caller]
                pub fn lock(&self) -> MutexGuard<'_, T, Lock> {
                    #[cfg(feature = "tracing")]
                    let location = Location::caller();

                    #[cfg(feature = "tracing")]
                    tracing::debug!(%location, "Mutex::lock");

                    let guard = self
                        .0
                        .lock()
                        .map(|guard| MutexGuard {
                            guard,

                            #[cfg(feature = "tracing")]
                            location,
                            _p: PhantomData,
                        })
                        .expect("loom mutex will never poison");

                    #[cfg(feature = "tracing")]
                    tracing::debug!(%location, "Mutex::lock -> locked");
                    guard
                }
            }

            impl<T: fmt::Debug, Lock> fmt::Debug for Mutex<T, Lock> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl<T, Lock> Deref for MutexGuard<'_, T, Lock> {
                type Target = T;
                #[inline]
                fn deref(&self) -> &Self::Target {
                    self.guard.deref()
                }
            }

            impl<T, Lock> DerefMut for MutexGuard<'_, T, Lock> {
                #[inline]
                fn deref_mut(&mut self) -> &mut Self::Target {
                    self.guard.deref_mut()
                }
            }

            impl<T: fmt::Debug, Lock> fmt::Debug for MutexGuard<'_, T, Lock> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.guard.fmt(f)
                }
            }

            impl<T, Lock> Drop for MutexGuard<'_, T, Lock> {
                #[track_caller]
                fn drop(&mut self) {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(
                        location.dropped = %Location::caller(),
                        location.locked = %self.location,
                        "MutexGuard::drop: unlocking",
                    );
                }
            }
        }
    }
}

#[cfg(not(loom))]
mod inner {
    #![allow(dead_code, unused_imports)]
    pub(crate) mod sync {
        #[cfg(any(feature = "alloc", test))]
        pub use alloc::sync::*;
        pub use core::sync::*;

        pub use crate::blocking;
    }

    pub(crate) mod atomic {
        pub use portable_atomic::*;
    }

    pub(crate) use portable_atomic::hint;

    #[cfg(test)]
    pub(crate) mod thread {

        pub(crate) use std::thread::{yield_now, JoinHandle};

        pub(crate) fn spawn<F, T>(f: F) -> JoinHandle<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            use super::atomic::{AtomicUsize, Ordering::Relaxed};
            thread_local! {
                static CHILDREN: AtomicUsize = const { AtomicUsize::new(1) };
            }

            let track = super::alloc::track::Registry::current();
            let subscriber = tracing::Dispatch::default();
            let span = tracing::Span::current();
            let num = CHILDREN.with(|children| children.fetch_add(1, Relaxed));
            std::thread::spawn(move || {
                let _tracing = tracing::dispatcher::set_default(&subscriber);
                let _span = tracing::info_span!(parent: span, "thread", message = num).entered();

                tracing::info!(num, "spawned child thread");
                let _tracking = track.map(|track| track.set_default());
                let res = f();
                tracing::info!(num, "child thread completed");

                res
            })
        }
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

            pub(crate) fn check(&self, f: impl FnOnce()) {
                let _trace = crate::util::test::trace_init();
                let _span = tracing::info_span!(
                    "test",
                    message = std::thread::current().name().unwrap_or("<unnamed>")
                )
                .entered();
                let registry = super::alloc::track::Registry::default();
                let _tracking = registry.set_default();

                tracing::info!("started test...");
                f();
                tracing::info!("test completed successfully!");

                registry.check();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn model(f: impl FnOnce()) {
        model::Builder::new().check(f)
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
            pub(crate) fn get(&self) -> ConstPtr<T> {
                ConstPtr(self.0.get())
            }

            #[inline(always)]
            pub(crate) fn get_mut(&self) -> MutPtr<T> {
                MutPtr(self.0.get())
            }
        }

        impl<T> UnsafeCell<T> {
            #[inline(always)]
            #[must_use]
            pub(crate) fn into_inner(self) -> T {
                self.0.into_inner()
            }
        }

        #[derive(Debug)]
        pub(crate) struct ConstPtr<T: ?Sized>(*const T);

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
        #[cfg(test)]
        use core::{
            future::Future,
            pin::Pin,
            task::{Context, Poll},
        };

        #[cfg(test)]
        use std::sync::Arc;
        #[cfg(test)]
        pub(in crate::loom) mod track {
            use std::{
                cell::RefCell,
                sync::{
                    atomic::{AtomicBool, Ordering},
                    Arc, Mutex, Weak,
                },
            };

            #[derive(Clone, Debug, Default)]
            pub(crate) struct Registry(Arc<Mutex<RegistryInner>>);

            #[derive(Debug, Default)]
            struct RegistryInner {
                tracks: Vec<Weak<TrackData>>,
                next_id: usize,
            }

            #[derive(Debug)]
            pub(super) struct TrackData {
                was_leaked: AtomicBool,
                type_name: &'static str,
                location: &'static core::panic::Location<'static>,
                id: usize,
            }

            thread_local! {
                static REGISTRY: RefCell<Option<Registry>> = const { RefCell::new(None) };
            }

            impl Registry {
                pub(in crate::loom) fn current() -> Option<Registry> {
                    REGISTRY.with(|current| current.borrow().clone())
                }

                pub(in crate::loom) fn set_default(&self) -> impl Drop {
                    struct Unset(Option<Registry>);
                    impl Drop for Unset {
                        fn drop(&mut self) {
                            let _ =
                                REGISTRY.try_with(|current| *current.borrow_mut() = self.0.take());
                        }
                    }

                    REGISTRY.with(|current| {
                        let mut current = current.borrow_mut();
                        let unset = Unset(current.clone());
                        *current = Some(self.clone());
                        unset
                    })
                }

                #[track_caller]
                pub(super) fn start_tracking<T>() -> Option<Arc<TrackData>> {
                    // we don't use `Option::map` here because it creates a
                    // closure, which breaks `#[track_caller]`, since the caller
                    // of `insert` becomes the closure, which cannot have a
                    // `#[track_caller]` attribute on it.
                    #[allow(clippy::manual_map)]
                    match Self::current() {
                        Some(registry) => Some(registry.insert::<T>()),
                        _ => None,
                    }
                }

                #[track_caller]
                pub(super) fn insert<T>(&self) -> Arc<TrackData> {
                    let mut inner = self.0.lock().unwrap();
                    let id = inner.next_id;
                    inner.next_id += 1;
                    let location = core::panic::Location::caller();
                    let type_name = std::any::type_name::<T>();
                    let data = Arc::new(TrackData {
                        type_name,
                        location,
                        id,
                        was_leaked: AtomicBool::new(false),
                    });
                    let weak = Arc::downgrade(&data);
                    tracing::trace!(
                        target: "maitake_sync::alloc",
                        id,
                        "type" = %type_name,
                        %location,
                        "started tracking allocation",
                    );
                    inner.tracks.push(weak);
                    data
                }

                pub(in crate::loom) fn check(&self) {
                    let leaked = self
                        .0
                        .lock()
                        .unwrap()
                        .tracks
                        .iter()
                        .filter_map(|weak| {
                            let data = weak.upgrade()?;
                            data.was_leaked.store(true, Ordering::SeqCst);
                            Some(format!(
                                " - id {}, {} allocated at {}",
                                data.id, data.type_name, data.location
                            ))
                        })
                        .collect::<Vec<_>>();
                    if !leaked.is_empty() {
                        let leaked = leaked.join("\n  ");
                        panic!("the following allocations were leaked:\n  {leaked}");
                    }
                }
            }

            impl Drop for TrackData {
                fn drop(&mut self) {
                    if !self.was_leaked.load(Ordering::SeqCst) {
                        tracing::trace!(
                            target: "maitake_sync::alloc",
                            id = self.id,
                            "type" = %self.type_name,
                            location = %self.location,
                            "dropped all references to a tracked allocation",
                        );
                    }
                }
            }
        }

        #[cfg(test)]
        #[derive(Debug)]
        #[pin_project::pin_project]
        pub(crate) struct TrackFuture<F> {
            #[pin]
            inner: F,
            track: Option<Arc<track::TrackData>>,
        }

        #[cfg(test)]
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

        #[cfg(test)]
        impl<F> TrackFuture<F> {
            /// Wrap a `Future` in a `TrackFuture` that participates in Loom's
            /// leak checking.
            #[track_caller]
            pub(crate) fn new(inner: F) -> Self {
                let track = track::Registry::start_tracking::<F>();
                Self { inner, track }
            }

            /// Stop tracking this future, and return the inner value.
            pub(crate) fn into_inner(self) -> F {
                self.inner
            }
        }

        #[cfg(test)]
        #[track_caller]
        pub(crate) fn track_future<F: Future>(inner: F) -> TrackFuture<F> {
            TrackFuture::new(inner)
        }

        // PartialEq impl so that `assert_eq!(..., Ok(...))` works
        #[cfg(test)]
        impl<F: PartialEq> PartialEq for TrackFuture<F> {
            fn eq(&self, other: &Self) -> bool {
                self.inner == other.inner
            }
        }

        /// Track allocations, detecting leaks
        #[derive(Debug, Default)]
        pub struct Track<T> {
            value: T,

            #[cfg(test)]
            track: Option<Arc<track::TrackData>>,
        }

        impl<T> Track<T> {
            /// Track a value for leaks
            #[inline(always)]
            #[track_caller]
            pub fn new(value: T) -> Track<T> {
                Track {
                    value,

                    #[cfg(test)]
                    track: track::Registry::start_tracking::<T>(),
                }
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
    pub(crate) mod future {
        pub(crate) use tokio_test::block_on;
    }

    #[cfg(test)]
    pub(crate) fn traceln(args: std::fmt::Arguments) {
        eprintln!("{args}");
    }

    #[cfg(not(test))]
    pub(crate) fn traceln(_: core::fmt::Arguments) {}
}
