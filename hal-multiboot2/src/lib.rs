#[repr(C)]
pub struct Header {
    magic: u32,
    arch: Arch,
    header_length: u32,
    checksum: u32,
    end_tag: Tag,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
enum Arch {
    X86 = 0,
    Mips = 4,
}

#[repr(C)]
#[derive(Debug)]
pub struct Tag {
    ty: TagType,
    len: u32,
}

pub trait TagType: crate::sealed::Sealed {
    const ID: usize;
}

/// Types of Multiboot tags
///
/// Refer to Chapter 3 of the Multiboot 2 spec
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TagType {
    /// Tag that indicates the end of multiboot tags
    End = 0,
    /// Command line passed to the bootloader
    CommandLine = 1,
    BootloaderName = 2,
    Modules = 3,
    BasicMemInfo = 4,
    BiosBootDev = 5,
    MemoryMap = 6,
    VbeInfo = 7,
    FramebufferInfo = 8,
    ElfSections = 9,
    ApmTable = 10,
}

mod sealed {
    pub trait Sealed {}
}
