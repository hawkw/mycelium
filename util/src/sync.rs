//! Synchronization primitives.

#[cfg(loom)]
pub use loom::sync::atomic;

#[cfg(not(loom))]
pub use core::sync::atomic;

pub mod once;
pub mod spin;
pub use self::once::{InitOnce, Lazy};

mod cache_pad;
pub use self::cache_pad::CachePadded;
pub mod hint {
    #[cfg(not(loom))]
    pub use core::hint::spin_loop;

    #[cfg(loom)]
    pub use loom::sync::atomic::spin_loop_hint as spin_loop;
}
