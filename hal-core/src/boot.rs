use crate::{mem, Architecture};
use core::iter::Iterator;

pub trait BootInfo {
    type Arch: Architecture;
    type MemoryMap: Iterator<Item = mem::Region<<Self::Arch as Architecture>::PAddr>>;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap;

    // TODO(eliza): figure out a non-bad way to represent boot command lines (is
    // it reasonable to convert them to rust strs when we barely have an operating
    // system???)
}
