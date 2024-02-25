#[allow(unused_imports)]
pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    #![allow(dead_code)]
    pub(crate) use loom::thread_local;

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
    #![allow(dead_code, unused_imports)]

    #[cfg(test)]
    pub(crate) use std::thread_local;

    pub(crate) mod sync {
        #[cfg(feature = "alloc")]
        pub use alloc::sync::*;
        pub use core::sync::*;

        pub use mycelium_util::sync::spin;
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
            let subscriber = tracing_02::Dispatch::default();
            let span = tracing_02::Span::current();
            let num = CHILDREN.with(|children| children.fetch_add(1, Relaxed));
            std::thread::spawn(move || {
                let _tracing = tracing_02::dispatch::set_default(&subscriber);
                let _span =
                    tracing_02::info_span!(parent: span.id(), "thread", message = num).entered();

                tracing_02::info!(num, "spawned child thread");
                let _tracking = track.map(|track| track.set_default());
                let res = f();
                tracing_02::info!(num, "child thread completed");

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
                let _span = tracing_02::info_span!(
                    "test",
                    message = std::thread::current().name().unwrap_or("<unnamed>")
                )
                .entered();
                let registry = super::alloc::track::Registry::default();
                let _tracking = registry.set_default();

                tracing_02::info!("started test...");
                f();
                tracing_02::info!("test completed successfully!");

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
                    trace!(
                        target: "maitake::alloc",
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
                        trace!(
                            target: "maitake::alloc",
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
