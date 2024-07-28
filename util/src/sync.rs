//! Synchronization primitives, and utilities for implementing them.

#[cfg(loom)]
pub use loom::sync::atomic;

#[cfg(not(loom))]
pub use core::sync::atomic;

pub mod cell;

pub use self::once::{InitOnce, Lazy};
pub use maitake_sync::blocking;
pub use maitake_sync::{spin::once, util::CachePadded};

/// Spinlocks and related synchronization primitives.
pub mod spin {
    pub use maitake_sync::{spin::*, util::Backoff};
}
/// A wrapper for the [`core::hint`] module that emits either [`loom`] spin loop
/// hints (when `cfg(loom)` is enabled), or real spin loop hints when loom is
/// not enabled.
///
/// [`loom`]: https://crates.io/crates/loom
pub mod hint {
    #[cfg(not(loom))]
    pub use core::hint::spin_loop;

    #[cfg(loom)]
    pub use loom::sync::atomic::spin_loop_hint as spin_loop;
}
