use crate::mem;
use core::iter::Iterator;

pub trait BootInfo {
    type MemoryMap: Iterator<Item = mem::Region>;
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

    fn init_paging(&self);
    // XXX(eliza): is this generic enough???
    // fn phys_mem_offset(&self) -> crate::VAddr;

    // TODO(eliza): figure out a non-bad way to represent boot command lines (is
    // it reasonable to convert them to rust strs when we barely have an operating
    // system???)
}
