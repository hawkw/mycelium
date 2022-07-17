//! Spinning-based synchronization primitives.
//!
//! The synchronization primitives in this module wait by spinning in a busy
//! loop, making them usable  in bare-metal environments.
mod backoff;
mod mutex;
pub use self::backoff::Backoff;
pub use self::mutex::*;
