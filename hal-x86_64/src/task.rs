use crate::{cpu, segment, VAddr};

/// A 64-bit mode task-state segment (TSS).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct StateSegment {
    _reserved_1: u32,
    /// The stack pointers for privilege levels 0-2.
    ///
    /// These addresses must be in canonical form.
    pub privilege_stacks: [VAddr; 3],
    _reserved_2: u64,
    /// The interrupt stack table.
    pub interrupt_stacks: [VAddr; 7],
    _reserved_3: u64,
    _reserved_4: u16,
    /// The 16-bit offset from the TSS' base address to the I/O permissions bitmap.
    pub iomap_offset: u16,
}

impl StateSegment {
    pub const fn empty() -> Self {
        Self {
            privilege_stacks: [VAddr::zero(); 3],
            interrupt_stacks: [VAddr::zero(); 7],
            iomap_offset: 0,
            _reserved_1: 0,
            _reserved_2: 0,
            _reserved_3: 0,
            _reserved_4: 0,
        }
    }

    /// Returns the virtual address of the I/O permission bitmap.
    #[inline]
    pub fn iomap_addr(&self) -> VAddr {
        VAddr::of(self).offset(self.iomap_offset as i32)
    }

    pub unsafe fn load_tss(sel: segment::Selector) {
        cpu::intrinsics::ltr(sel);
    }
}

impl Default for StateSegment {
    fn default() -> Self {
        Self::empty()
    }
}
