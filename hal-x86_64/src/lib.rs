//! Implementation of the Mycelium HAL for 64-bit x86 platforms.
#![cfg_attr(not(test), no_std)]
#![feature(asm)]
#![feature(abi_x86_interrupt)]
// Oftentimes it's necessary to write to a value at a particular location in
// memory, and these types don't implement Copy to ensure they aren't
// inadvertantly copied.
#![allow(clippy::trivially_copy_pass_by_ref)]

use core::{fmt, ops};
use hal_core::{Address, Architecture};
pub(crate) use hal_core::{PAddr, VAddr};
pub mod cpu;
pub mod interrupt;
pub mod segment;
pub mod serial;
pub mod tracing;
pub mod vga;

#[derive(Debug)]
pub struct X64;

impl Architecture for X64 {
    const NAME: &'static str = "x86_64";
    type InterruptCtrl = crate::interrupt::Idt;

    fn init_interrupts(bootinfo: &impl hal_core::boot::BootInfo) -> &'static mut Self::InterruptCtrl
    where
        Self: Sized,
    {
        crate::interrupt::init(bootinfo)
    }
}
