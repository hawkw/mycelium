use crate::{mem, Architecture};
use core::iter::Iterator;

pub trait BootInfo {
    type Arch: Architecture;
    type MemoryMap: Iterator<Item = mem::Region<Self::Arch>>;
    type Writer: core::fmt::Write;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap;

    /// Returns a writer for printing early kernel diagnostics
    fn writer(&self) -> Self::Writer;

    fn bootloader_name(&self) -> &str;

    fn bootloader_version(&self) -> Option<&str> {
        None
    }

    fn subscriber(&self) -> Option<tracing::Dispatch> {
        None
    }

    // TODO(eliza): figure out a non-bad way to represent boot command lines (is
    // it reasonable to convert them to rust strs when we barely have an operating
    // system???)
}
