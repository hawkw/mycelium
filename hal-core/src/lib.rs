#![cfg_attr(target_os = "none", no_std)]
use core::{fmt, ops};
mod addr;
pub mod boot;
pub mod interrupt;
pub mod mem;
pub use self::addr::*;
pub use self::boot::BootInfo;

pub trait Architecture {
    type InterruptCtrl: interrupt::Control + 'static;

    /// The name of the architecture, as a string.
    const NAME: &'static str;

    fn init_interrupts(bootinfo: &impl boot::BootInfo) -> &'static mut Self::InterruptCtrl
    where
        Self: Sized;
}
