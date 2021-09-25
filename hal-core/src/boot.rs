use crate::mem;
use core::iter::Iterator;

pub trait BootInfo {
    type MemoryMap: Iterator<Item = mem::Region>;
    type Writer: core::fmt::Write;
    type Framebuffer: crate::framebuffer::Draw;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap;

    /// Returns a writer for printing early kernel diagnostics
    fn writer(&self) -> Self::Writer;

    fn framebuffer(&self) -> Option<Self::Framebuffer>;

    fn bootloader_name(&self) -> &str;

    fn bootloader_version(&self) -> Option<&str> {
        None
    }

    fn subscriber(&self) -> Option<tracing::Dispatch> {
        None
    }

    fn init_paging(&self);

    // TODO(eliza): figure out a non-bad way to represent boot command lines (is
    // it reasonable to convert them to rust strs when we barely have an operating
    // system???)
}
