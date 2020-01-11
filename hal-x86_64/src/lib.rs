//! Implementation of the Mycelium HAL for 64-bit x86 platforms.
#![cfg_attr(not(test), no_std)]
#![feature(asm)]
#![feature(abi_x86_interrupt)]
// Oftentimes it's necessary to write to a value at a particular location in
// memory, and these types don't implement Copy to ensure they aren't
// inadvertantly copied.
#![allow(clippy::trivially_copy_pass_by_ref)]

use core::{fmt, ops};
pub(crate) use hal_core::{PAddr, VAddr};

pub mod control_regs;
pub mod cpu;
pub mod interrupt;
pub mod mm;
pub mod segment;
pub mod serial;
pub mod tracing;
pub mod vga;

pub const NAME: &'static str = "x86_64";
