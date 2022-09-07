//! Time utilities.
pub mod timeout;
pub mod timer;

pub use self::timeout::Timeout;
pub use self::timer::{sleep::Sleep, Timer};
pub use core::time::Duration;
