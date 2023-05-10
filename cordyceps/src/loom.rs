pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    #![allow(unused_imports)]

    pub(crate) mod atomic {
        pub use core::sync::atomic::Ordering;
        pub use loom::sync::atomic::*;
    }

    pub(crate) use loom::{cell, hint, model, sync, thread};

    pub(crate) mod alloc {
        #![allow(dead_code)]
        use core::fmt;
        use loom::alloc;
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

#[cfg(not(loom))]
mod inner {
    #![allow(dead_code)]
    pub(crate) mod sync {
        pub use core::sync::*;

        #[cfg(all(feature = "alloc", not(test)))]
        pub use alloc::sync::*;

        #[cfg(test)]
        pub use std::sync::*;
    }

    pub(crate) use core::sync::atomic;

    #[cfg(test)]
    pub(crate) mod thread {
        pub(crate) use std::thread::{yield_now, JoinHandle};
        pub(crate) fn spawn<F, T>(f: F) -> JoinHandle<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            let track = super::alloc::track::Registry::current();
            std::thread::spawn(move || {
                let _tracking = track.map(|track| track.set_default());
                f()
            })
        }
    }
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
                let registry = super::alloc::track::Registry::default();
                let _tracking = registry.set_default();
                f();
                registry.check();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn model(f: impl FnOnce()) {
        let collector = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .without_time()
            .with_thread_ids(true)
            .with_thread_names(false)
            .finish();
        let _ = tracing::subscriber::set_global_default(collector);
        model::Builder::new().check(f)
    }

    pub(crate) mod alloc {
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
                static REGISTRY: RefCell<Option<Registry>> = RefCell::new(None);
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
                    test_trace!(
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
                        test_trace!(
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

        /// Track allocations, detecting leaks
        #[derive(Debug, Default)]
        pub struct Track<T> {
            value: T,

            #[cfg(test)]
            track: Option<Arc<track::TrackData>>,
        }

        impl<T> Track<T> {
            pub const fn new_const(value: T) -> Track<T> {
                Track {
                    value,

                    #[cfg(test)]
                    track: None,
                }
            }

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
}
