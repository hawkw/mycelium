use crate::Architecture;

pub mod page;

/// A cross-platform representation of a memory region.
#[derive(Debug, Clone)]
pub struct Region<A: Architecture> {
    base: A::PAddr,
    // TODO(eliza): should regions be stored as (start -> end) or as
    // (base + offset)?
    size: usize,
    kind: RegionKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct RegionKind(KindInner);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
enum KindInner {
    /// For whatever reason, this memory region's kind is undetermined.
    // TODO(eliza): do we need this?
    Unknown = 0,
    /// Free memory
    Free,
    /// Memory used by the kernel
    Used,
    /// Memory containing bootloader info that may be reclaimed by the kernel.
    ///
    /// The kernel is responsible for ensuring that any required information
    /// from the bootloader is consumed before reclaiming this memory.
    ///
    /// This may include, for example, ACPI tables.
    BootReclaimable,
    /// Memory containing bootloader info that may **not** be reclaimed by the
    /// kernel.
    Boot,
    /// Bad memory.
    Bad,
    /// Kernel memory
    Kernel,
    /// Kernel stack
    KernelStack,
    /// Memory used for a page table.
    PageTable,
}

impl<A: Architecture> Region<A> {
    /// Returns the base address of the memory region
    pub fn base_addr(&self) -> A::PAddr {
        self.base
    }

    /// Returns the size (in bytes) of the memory region.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns `true` if `self` contains the specified address.
    pub fn contains(&self, addr: impl Into<A::PAddr>) -> bool {
        let addr = addr.into();
        addr > self.base && addr < self.end_addr()
    }

    /// Returns the end address of the memory region.
    pub fn end_addr(&self) -> A::PAddr {
        self.base + self.size
    }

    pub fn kind(&self) -> RegionKind {
        self.kind
    }

    pub fn new(base: A::PAddr, size: usize, kind: RegionKind) -> Self {
        Self { base, size, kind }
    }

    pub fn page_range<S: page::Size>(&self) -> page::PageRange<A::PAddr, S> {
        unimplemented!("eliza")
    }
}

impl RegionKind {
    /// Free memory
    pub const FREE: Self = Self(KindInner::Free);

    /// Memory used by the kernel
    pub const USED: Self = Self(KindInner::Used);

    /// Memory containing bootloader info that may be reclaimed by the kernel.
    ///
    /// The kernel is responsible for ensuring that any required information
    /// from the bootloader is consumed before reclaiming this memory.
    ///
    /// This may include, for example, ACPI tables.
    pub const BOOT_RECLAIMABLE: Self = Self(KindInner::BootReclaimable);

    /// Memory containing bootloader info that may **not** be reclaimed by the
    /// kernel.
    pub const BOOT: Self = Self(KindInner::Boot);

    /// Bad memory
    pub const BAD: Self = Self(KindInner::Bad);

    /// Kernel memory
    pub const KERNEL: Self = Self(KindInner::Kernel);

    /// Kernel stack
    pub const KERNEL_STACK: Self = Self(KindInner::KernelStack);

    /// Memory used for a page table.
    pub const PAGE_TABLE: Self = Self(KindInner::PageTable);
}
