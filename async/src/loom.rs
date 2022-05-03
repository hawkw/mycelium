#[allow(unused_imports)]
pub(crate) use self::inner::*;

#[cfg(loom)]
mod inner {
    pub use loom::{alloc, hint, sync, thread};
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
        use tracing_subscriber_03::{
            filter::{LevelFilter, Targets},
            fmt,
            prelude::*,
        };
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
                .unwrap_or_else(|| Targets::new().with_target("loom", LevelFilter::INFO));
            fmt::Subscriber::builder()
                .with_writer(|| TracebufWriter)
                .without_time()
                .with_max_level(LevelFilter::TRACE)
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
            // clear the buffer for the next iteration...
            TRACE_BUF.with(|buf| buf.borrow_mut().clear());

            let iteration = current_iteration.fetch_add(1, Ordering::Relaxed);
            traceln(format_args!(
                "\n---- {} iteration {} ----",
                test_name, iteration,
            ));

            model();
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
}

#[cfg(not(loom))]
mod inner {
    #![allow(dead_code)]

    pub use core::sync;

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
