#[cfg(target_arch = "x86_64")]
pub(crate) mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use self::x86_64::*;
