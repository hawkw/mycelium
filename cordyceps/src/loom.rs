pub(crate) use self::inner::*;

#[cfg(all(test, loom))]
mod inner {

    pub(crate) mod atomic {
        pub use loom::sync::atomic::*;
        pub use std::sync::atomic::Ordering;
    }

    pub(crate) use loom::{cell, future, hint, sync, thread};
    use std::{cell::RefCell, fmt::Write};

    pub(crate) mod model {
        #[allow(unused_imports)]
        pub(crate) use loom::model::Builder;
    }

    std::thread_local! {
        static TRACE_BUF: RefCell<String> = RefCell::new(String::new());
    }

    pub(crate) fn traceln(args: std::fmt::Arguments) {
        let mut args = Some(args);
        TRACE_BUF
            .try_with(|buf| {
                let mut buf = buf.borrow_mut();
                let _ = buf.write_fmt(args.take().unwrap());
                let _ = buf.write_char('\n');
            })
            .unwrap_or_else(|_| println!("{}", args.take().unwrap()))
    }

    #[track_caller]
    pub(crate) fn run_builder(
        builder: loom::model::Builder,
        model: impl Fn() + Sync + Send + std::panic::UnwindSafe + 'static,
    ) {
        use std::{
            env, io,
            sync::{
                atomic::{AtomicBool, AtomicUsize, Ordering},
                Once,
            },
        };
        use tracing_subscriber::{filter::Targets, fmt, prelude::*};
        static IS_NOCAPTURE: AtomicBool = AtomicBool::new(false);
        static SETUP_TRACE: Once = Once::new();

        SETUP_TRACE.call_once(|| {
            // set up tracing for loom.
            const LOOM_LOG: &str = "LOOM_LOG";

            struct TracebufWriter;
            impl io::Write for TracebufWriter {
                fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                    let len = buf.len();
                    let s = std::str::from_utf8(buf)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                    TRACE_BUF.with(|buf| buf.borrow_mut().push_str(s));
                    Ok(len)
                }

                fn flush(&mut self) -> io::Result<()> {
                    Ok(())
                }
            }

            let filter = env::var(LOOM_LOG)
                .ok()
                .and_then(|var| match var.parse::<Targets>() {
                    Err(e) => {
                        eprintln!("invalid {}={:?}: {}", LOOM_LOG, var, e);
                        None
                    }
                    Ok(targets) => Some(targets),
                })
                .unwrap_or_else(|| Targets::new().with_target("loom", tracing::Level::INFO));
            fmt::Subscriber::builder()
                .with_writer(|| TracebufWriter)
                .without_time()
                .with_max_level(tracing::Level::TRACE)
                .finish()
                .with(filter)
                .init();

            if std::env::args().any(|arg| arg == "--nocapture") {
                IS_NOCAPTURE.store(true, Ordering::Relaxed);
            }

            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic| {
                // try to print the trace buffer.
                TRACE_BUF
                    .try_with(|buf| {
                        if let Ok(mut buf) = buf.try_borrow_mut() {
                            eprint!("{}", buf);
                            buf.clear();
                        } else {
                            eprint!("trace buf already mutably borrowed?");
                        }
                    })
                    .unwrap_or_else(|e| eprintln!("trace buf already torn down: {}", e));

                // let the default panic hook do the rest...
                default_hook(panic);
            }))
        });

        // wrap the loom model with `catch_unwind` to avoid potentially losing
        // test output on double panics.
        let current_iteration = std::sync::Arc::new(AtomicUsize::new(1));
        let iteration = current_iteration.clone();
        let test_name = match std::thread::current().name() {
            Some("main") | None => "test".to_string(),
            Some(name) => name.to_string(),
        };
        builder.check(move || {
            let iteration = current_iteration.fetch_add(1, Ordering::Relaxed);
            traceln(format_args!(
                "\n---- {} iteration {} ----",
                test_name, iteration,
            ));

            model();
            // if this iteration succeeded, clear the buffer for the
            // next iteration...
            TRACE_BUF.with(|buf| buf.borrow_mut().clear());
        });

        // Only print iterations on test completion in nocapture mode; otherwise
        // they'll just get all mangled.
        if IS_NOCAPTURE.load(Ordering::Relaxed) {
            print!("({} iterations) ", iteration.load(Ordering::Relaxed));
        }
    }

    #[track_caller]
    pub(crate) fn model(model: impl Fn() + std::panic::UnwindSafe + Sync + Send + 'static) {
        run_builder(Default::default(), model)
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

#[cfg(not(all(loom, test)))]
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

    #[cfg(feature = "std")]
    pub(crate) fn traceln(args: std::fmt::Arguments) {
        eprintln!("{}", args);
    }

    #[cfg(not(feature = "std"))]
    pub(crate) fn traceln(_: core::fmt::Arguments) {}
}
