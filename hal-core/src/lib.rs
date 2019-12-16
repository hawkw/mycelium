#![cfg_attr(target_os = "none", no_std)]
use core::{fmt, ops};
pub mod boot;
pub mod interrupt;
pub mod mem;

pub trait Architecture {
    /// The architecture's physical address representation.
    type PAddr: Address;

    /// The name of the architecture, as a string.
    const NAME: &'static str;
}

pub trait Address:
    Copy
    + ops::Add<usize, Output = Self>
    + ops::Sub<usize, Output = Self>
    + ops::AddAssign<usize>
    + ops::SubAssign<usize>
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + fmt::Debug
{
    fn as_usize(self) -> usize;

    /// Aligns `self` up to `align`.
    ///
    /// The specified alignment must be a power of two.
    fn align_up<A: Into<usize>>(self, align: A) -> Self;

    /// Aligns `self` down to `align`.
    ///
    /// The specified alignment must be a power of two.
    fn align_down<A: Into<usize>>(self, align: A) -> Self;

    /// Offsets this address by `offset`.
    ///
    /// If the specified offset would overflow, this function saturates instead.
    fn offset(self, offset: i32) -> Self {
        if offset > 0 {
            self + offset as usize
        } else {
            let offset = offset * -1;
            self - offset as usize
        }
    }

    /// Returns the difference between `self` and `other`.
    fn difference(self, other: Self) -> isize {
        if self > other {
            (self.as_usize() - other.as_usize()) as isize * -1
        } else {
            (other.as_usize() - self.as_usize()) as isize
        }
    }

    /// Returns `true` if `self` is aligned on the specified alignment.
    fn is_aligned<A: Into<usize>>(self, align: A) -> bool {
        self.align_down(align) == self
    }

    fn as_ptr(&self) -> *const ();
}

pub use self::boot::BootInfo;
