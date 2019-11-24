#![no_std]
use core::ops;
mod boot;
pub mod mem;

pub trait Architecture {
    /// The architecture's physical address representation.
    type PAddr: Address;

    /// The name of the architecture, as a string.
    const NAME: &'static str;
}

pub trait Address:
    Copy + ops::Add<usize> + ops::Sub<usize> + ops::AddAssign<usize> + ops::SubAssign<usize>
{
    // TODO(eliza): more ops (align up/down?)
}

pub use self::boot::BootInfo;
