#![cfg_attr(target_os = "none", no_std)]
#![feature(try_trait)]

use core::{fmt, ops};
mod addr;

pub mod boot;
pub mod interrupt;
pub mod mem;
pub use self::addr::*;
pub use self::boot::BootInfo;
