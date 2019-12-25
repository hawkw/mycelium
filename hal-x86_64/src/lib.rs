//! Implementation of the Mycelium HAL for 64-bit x86 platforms.
#![cfg_attr(not(test), no_std)]
use core::{fmt, ops};
use hal_core::{Address, Architecture};

pub mod vga;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PAddr(u64);

#[derive(Debug)]
pub struct X64;

impl Architecture for X64 {
    type PAddr = PAddr;
    const NAME: &'static str = "x86_64";
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
