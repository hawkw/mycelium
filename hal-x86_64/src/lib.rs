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

pub mod cpu;
pub mod interrupt;
pub mod log;
pub mod segment;
pub mod serial;
pub mod vga;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PAddr(u64);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VAddr(u64);

#[derive(Debug)]
pub struct X64;

impl Architecture for X64 {
    type PAddr = PAddr;
    type VAddr = VAddr;
    const NAME: &'static str = "x86_64";
    type InterruptCtrl = crate::interrupt::Idt;

    fn init_interrupts<H, B>(bootinfo: &B) -> &'static mut Self::InterruptCtrl
    where
        Self: Sized,
        H: hal_core::interrupt::Handlers<Self>,
        B: hal_core::boot::BootInfo<Arch = Self>,
    {
        crate::interrupt::init::<H, _>(bootinfo)
    }
}

impl fmt::Debug for PAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(width) = f.width() {
            f.debug_tuple("PAddr")
                .field(&format_args!("{:#0width$x}", self.0, width = width))
                .finish()
        } else {
            f.debug_tuple("PAddr")
                .field(&format_args!("{:#x}", self.0,))
                .finish()
        }
    }
}

impl ops::Add<usize> for PAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        PAddr(self.0 + rhs as u64)
    }
}

impl ops::Add for PAddr {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        PAddr(self.0 + rhs.0)
    }
}

impl ops::AddAssign for PAddr {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl ops::AddAssign<usize> for PAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs as u64;
    }
}

impl ops::Sub<usize> for PAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        PAddr(self.0 - rhs as u64)
    }
}

impl ops::Sub for PAddr {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        PAddr(self.0 - rhs.0)
    }
}

impl ops::SubAssign for PAddr {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl ops::SubAssign<usize> for PAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs as u64;
    }
}

impl Address for PAddr {
    fn as_usize(self) -> usize {
        self.0 as usize
    }

    fn align_up<A: Into<usize>>(self, _align: A) -> Self {
        unimplemented!("eliza")
    }

    /// Aligns `self` down to `align`.
    ///
    /// The specified alignment must be a power of two.
    fn align_down<A: Into<usize>>(self, _align: A) -> Self {
        unimplemented!("eliza")
    }

    /// Returns `true` if `self` is aligned on the specified alignment.
    fn is_aligned<A: Into<usize>>(self, _align: A) -> bool {
        unimplemented!("eliza")
    }

    fn as_ptr(&self) -> *const () {
        unimplemented!("eliza")
    }
}

impl PAddr {
    pub fn from_u64(u: u64) -> Self {
        // TODO(eliza): ensure that this is a valid physical address?
        PAddr(u)
    }
}

impl ops::Add<usize> for VAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        VAddr(self.0 + rhs as u64)
    }
}

impl ops::Add for VAddr {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        VAddr(self.0 + rhs.0)
    }
}

impl ops::AddAssign for VAddr {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl ops::AddAssign<usize> for VAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs as u64;
    }
}

impl ops::Sub<usize> for VAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        VAddr(self.0 - rhs as u64)
    }
}

impl ops::Sub for VAddr {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        VAddr(self.0 - rhs.0)
    }
}

impl ops::SubAssign for VAddr {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl ops::SubAssign<usize> for VAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs as u64;
    }
}

impl Address for VAddr {
    fn as_usize(self) -> usize {
        self.0 as usize
    }

    fn align_up<A: Into<usize>>(self, _align: A) -> Self {
        unimplemented!("eliza")
    }

    /// Aligns `self` down to `align`.
    ///
    /// The specified alignment must be a power of two.
    fn align_down<A: Into<usize>>(self, _align: A) -> Self {
        unimplemented!("eliza")
    }

    /// Returns `true` if `self` is aligned on the specified alignment.
    fn is_aligned<A: Into<usize>>(self, _align: A) -> bool {
        unimplemented!("eliza")
    }

    fn as_ptr(&self) -> *const () {
        unimplemented!("eliza")
    }
}

impl VAddr {
    pub fn from_u64(u: u64) -> Self {
        // TODO(eliza): ensure that this is a valid virtual address?
        VAddr(u)
    }
}

impl fmt::Debug for VAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(width) = f.width() {
            f.debug_tuple("VAddr")
                .field(&format_args!("{:#0width$x}", self.0, width = width))
                .finish()
        } else {
            f.debug_tuple("VAddr")
                .field(&format_args!("{:#x}", self.0,))
                .finish()
        }
    }
}
