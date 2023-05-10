#![allow(unused_unsafe)]
#![cfg_attr(target_os = "none", no_std)]
#![feature(doc_cfg)]
mod addr;

extern crate alloc;

pub mod boot;
pub mod framebuffer;
pub mod interrupt;
pub mod mem;
mod local;
pub use self::addr::*;
pub use self::boot::BootInfo;
pub use self::local::CoreLocal;
