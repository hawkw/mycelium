#![allow(unused_unsafe)]
#![cfg_attr(target_os = "none", no_std)]
#![feature(doc_cfg)]
mod addr;

pub mod boot;
pub mod framebuffer;
pub mod interrupt;
pub mod mem;
pub use self::addr::*;
pub use self::boot::BootInfo;
