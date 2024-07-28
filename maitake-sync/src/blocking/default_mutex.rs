//! Default "chef's choice" [`mutex_traits::ScopedRawMutex`] implementation.
//!
//! This type is what users will get when they don't override the `Lock` type
//! parameter for `maitake-sync`'s synchronziation primitives. Therefore, we
//! make a best-effort attempt to Do The Right Thing based on the current
//! feature flag combination. In particular, here's what we currently give you:
//!
//! - **If `cfg(loom)` is enabled, then the `DefaultMutex` is a `loom` mutex**
//!   so that `maitake-sync` primitives work nicely in `loom` tests
//!
//! - **If the `std` feature is enabled, then the `DefaultMutex` is a
//!   `std::sync::Mutex`**, so that `std` users get an OS mutex rather than a
//!   spinlock.
//!
//! - **If the `critical-section` feature is enabled, then the `DefaultMutex` is
//!   a spinlock that acquires a critical section once locked.**. This ensures
//!   that bare-metal users who have enabled `critical-section` get a mutex that
//!   disables IRQs when locked.
//!
//! - **Otherwise, the `DefaultMutex` is a spinlock**. This is the default
//!   behavior and will at least work on all platforms, but may not be the most
//!   efficient, and may not be IRQ-safe.
//!
//!
//! # Notes
//!
//! - The `DefaultMutex` cannot ever implement `RawMutex`, only
//!   `ScopedRawMutex`. This is because it's impossible for a
//!   `critical-section`-based implementation to implement `RawMutex`, due to
//!   [`critical-section`'s safety requirements][cs-reqs], which we can't uphold
//!   in a RAII situation with multiple locks. If we implemented `RawMutex` for
//!   the non-`critical-section` implementations, then the `critical-section`
//!   feature flag would *take away* methods that would otherwise be available,
//!   making it non-additive, which is a BIG NO-NO for feature flags.
//!
//! - On the other hand, it *is* okay to have `cfg(loom)` not implement
//!   `ConstInit` where every other implementation does. This is because `cfg(loom)`
//!   is a `RUSTFLAGS` cfg rather than a feature flag, and therefore can only be
//!   enabled by the top-level build. It can't be enabled by a dependency and
//!   suddenly make your code not compile. Loom users are already used to stuff
//!   being const-initializable in real life, but not in loom tests, so this is
//!   more okay.
//!
//! [cs-reqs]: https://docs.rs/critical-section/latest/critical_section/fn.acquire.html#safety
#[cfg(loom)]
pub use loom_impl::DefaultMutex;

#[cfg(all(not(loom), feature = "std"))]
pub use std_impl::DefaultMutex;

#[cfg(all(not(loom), not(feature = "std"), feature = "critical-section"))]
pub use cs_impl::DefaultMutex;

#[cfg(all(not(loom), not(feature = "std"), not(feature = "critical-section")))]
pub use spin_impl::DefaultMutex;

#[cfg(loom)]
mod loom_impl {
    #[cfg(feature = "tracing")]
    use core::panic::Location;
    use mutex_traits::ScopedRawMutex;

    pub struct DefaultMutex(loom::sync::Mutex<()>);

    unsafe impl ScopedRawMutex for DefaultMutex {
        #[track_caller]
        fn with_lock<R>(&self, f: impl FnOnce() -> R) -> R {
            #[cfg(feature = "tracing")]
            let location = Location::caller();
            #[cfg(feature = "tracing")]
            tracing::debug!(%location, "DefaultMutex::with_lock()");

            let guard = self.0.lock();
            tracing::debug!(%location, "DefaultMutex::with_lock() -> locked");

            let result = f();
            drop(guard);

            #[cfg(feature = "tracing")]
            tracing::debug!(%location, "DefaultMutex::with_lock() -> unlocked");

            result
        }

        #[track_caller]
        fn try_with_lock<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
            #[cfg(feature = "tracing")]
            let location = Location::caller();
            #[cfg(feature = "tracing")]
            tracing::debug!(%location, "DefaultMutex::try_with_lock()");

            match self.0.try_lock() {
                Ok(guard) => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(%location, "DefaultMutex::try_with_lock() -> locked");

                    let result = f();
                    drop(guard);

                    #[cfg(feature = "tracing")]
                    tracing::debug!(%location, "DefaultMutex::try_with_lock() -> unlocked");

                    Some(result)
                }
                None => {
                    #[cfg(feature = "tracing")]
                    tracing::debug!(%location, "DefaultMutex::try_with_lock() -> already locked");

                    None
                }
            }
        }

        fn is_locked(&self) -> bool {
            self.0.try_lock().is_none()
        }
    }
}

#[cfg(all(not(loom), feature = "std"))]
mod std_impl {
    use mutex_traits::ScopedRawMutex;

    pub struct DefaultMutex(std::sync::Mutex<()>);

    unsafe impl ScopedRawMutex for DefaultMutex {
        #[track_caller]
        #[inline]
        fn with_lock<R>(&self, f: impl FnOnce() -> R) -> R {
            let _guard = self.0.lock().unwrap();
            f()
        }

        #[track_caller]
        fn try_with_lock<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
            let _guard = self.0.try_lock().ok()?;
            Some(f())
        }

        #[inline]
        fn is_locked(&self) -> bool {
            self.0.try_lock().is_ok()
        }
    }
}

#[cfg(all(not(loom), not(feature = "std"), feature = "critical-section"))]
mod cs_impl {
    use crate::spin::Spinlock;
    use mutex_traits::ScopedRawMutex;

    pub struct DefaultMutex(Spinlock);

    unsafe impl ScopedRawMutex for DefaultMutex {
        #[track_caller]
        #[inline]
        fn with_lock<R>(&self, f: impl FnOnce() -> R) -> R {
            self.0.with_lock(|| critical_section::with(|| f()))
        }

        #[track_caller]
        #[inline]
        fn try_with_lock<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
            self.0.try_with_lock(|| critical_section::with(|| f()))
        }

        #[inline]
        fn is_locked(&self) -> bool {
            self.0.is_locked()
        }
    }
}

#[cfg(all(not(loom), not(feature = "std"), not(feature = "critical-section")))]
mod spin_impl {
    use crate::spin::Spinlock;
    use mutex_traits::ScopedRawMutex;

    pub struct DefaultMutex(Spinlock);

    unsafe impl ScopedRawMutex for DefaultMutex {
        #[track_caller]
        #[inline]
        fn with_lock<R>(&self, f: impl FnOnce() -> R) -> R {
            self.0.with_lock(|| critical_section::with(|| f()))
        }

        #[track_caller]
        #[inline]
        fn try_with_lock<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
            self.0.try_with_lock(|| critical_section::with(|| f()))
        }

        #[inline]
        fn is_locked(&self) -> bool {
            self.0.is_locked()
        }
    }
}
