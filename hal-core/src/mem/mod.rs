use crate::Address;
use core::{cmp, fmt};
pub mod page;

/// A cross-platform representation of a memory region.
#[derive(Clone)]
pub struct Region<A = crate::PAddr> {
    base: A,
    // TODO(eliza): should regions be stored as (start -> end) or as
    // (base + offset)?
    size: usize,
    kind: RegionKind,
}

#[derive(Copy, Clone, Eq, PartialEq)]
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

impl<A: Address> Region<A> {
    pub fn new(base: A, size: usize, kind: RegionKind) -> Self {
        Self { base, size, kind }
    }

    /// Returns the base address of the memory region
    pub fn base_addr(&self) -> A {
        self.base
    }

    /// Returns the size (in bytes) of the memory region.
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn is_aligned(&self, align: impl Into<usize>) -> bool {
        self.base.is_aligned(align)
    }

    /// Returns `true` if `self` contains the specified address.
    pub fn contains(&self, addr: impl Into<A>) -> bool {
        let addr = addr.into();
        addr >= self.base && addr < self.end_addr()
    }

    /// Returns the end address (exclusive) of the memory region.
    ///
    /// This is the start address of the next memory region.
    pub fn end_addr(&self) -> A {
        self.base + self.size
    }

    pub fn kind(&self) -> RegionKind {
        self.kind
    }

    pub fn split_front(&mut self, size: usize) -> Option<Self> {
        assert!(size <= core::i32::MAX as usize);
        if size > self.size {
            return None;
        }
        let base = self.base;
        tracing::trace!(size, self.size, "splitting down by");
        self.base = self.base.offset(size as i32);
        self.size -= size;
        Some(Self {
            base,
            size,
            kind: self.kind,
        })
    }

    pub fn split_back(&mut self, size: usize) -> Option<Self> {
        assert!(size <= core::i32::MAX as usize);
        if size >= self.size {
            return None;
        }
        let rem_size = self.size - size;
        let base = self.base + size;
        tracing::trace!(
            size,
            self.size,
            ?self.base,
            rem_size,
            ?base,
            "split_back"
        );
        self.size = size;
        Some(Self {
            base,
            size: rem_size,
            kind: self.kind,
        })
    }

    pub fn page_range<S: page::Size>(
        &self,
        size: S,
    ) -> Result<page::PageRange<A, S>, page::NotAligned<S>> {
        tracing::trace!(?self.base, self.size, self.end = ?self.end_addr());
        let start = page::Page::starting_at(self.base, size)?;
        let end = page::Page::starting_at(self.end_addr(), size)?;
        Ok(start.range_to(end))
    }

    pub fn from_page_range<S: page::Size>(range: page::PageRange<A, S>, kind: RegionKind) -> Self {
        let base = range.start().base_addr();
        Self {
            base,
            size: range.size(),
            kind,
        }
    }

    pub fn merge(&mut self, other: &mut Self) {
        debug_assert_eq!(self.kind, other.kind);
        // TOOD(eliza): assert that there's no in-between space.
        self.base = cmp::min(self.base, other.base);
        self.size = self.size + other.size;
    }
}

impl<A> fmt::Debug for Region<A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Region")
            .field("base", &self.base)
            .field("size", &self.size)
            .field("kind", &self.kind)
            .finish()
    }
}

impl RegionKind {
    pub const UNKNOWN: Self = Self(KindInner::Unknown);

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

impl fmt::Debug for RegionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            RegionKind::FREE => f.pad("FREE"),
            RegionKind::USED => f.pad("USED"),
            RegionKind::BOOT => f.pad("BOOT"),
            RegionKind::BOOT_RECLAIMABLE => f.pad("BOOT_RECLAIMABLE"),
            RegionKind::BAD => f.pad("BAD T_T"),
            RegionKind::KERNEL => f.pad("KERNEL"),
            RegionKind::KERNEL_STACK => f.pad("KERNEL_STACK"),
            RegionKind::PAGE_TABLE => f.pad("PAGE_TABLE"),
            _ => f.pad("UNKNOWN ?_?"),
        }
    }
}
