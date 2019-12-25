use crate::cpu;

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Selector(u16);

/// Returns the current code segment selector in `%cs`.
pub fn code_segment() -> Selector {
    let value: u16;
    unsafe { asm!("mov %cs, $0" : "=r" (value)) };
    Selector(value)
}

impl Selector {
    const RING_BITS: u16 = cpu::Ring::Ring3 as u16;
    const INDEX_BITS: u16 = !(Self::RING_BITS | Self::TI_LDT);
    const INDEX_SHIFT: u16 = Self::INDEX_BITS.trailing_zeros() as u16;
    const TI_LDT: u16 = 0b100;

    pub const fn null() -> Self {
        Self(0)
    }

    pub const fn from_index(u: u16) -> Self {
        Self(u << Self::INDEX_SHIFT)
    }

    pub const fn from_raw(u: u16) -> Self {
        Self(u)
    }

    pub fn ring(self) -> cpu::Ring {
        cpu::Ring::from_u8((self.0 & Self::RING_BITS) as u8)
    }

    /// Returns true if this is an LDT segment selector.
    pub const fn is_ldt(&self) -> bool {
        self.0 & Self::TI_LDT == Self::TI_LDT
    }

    /// Returns true if this is a GDT segment selector.
    pub const fn is_gdt(&self) -> bool {
        !self.is_ldt()
    }

    /// Returns the index into the LDT or GDT this selector refers to.
    pub const fn index(&self) -> u16 {
        self.0 & Self::INDEX_BITS >> Self::INDEX_SHIFT
    }

    pub fn set_gdt(&mut self) -> &mut Self {
        self.0 &= !Self::TI_LDT;
        self
    }

    pub fn set_ldt(&mut self) -> &mut Self {
        self.0 |= Self::TI_LDT;
        self
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        self.0 &= !Self::RING_BITS | ring as u16;
        self
    }

    pub fn set_index(&mut self, index: u16) -> &mut Self {
        self.0 &= !Self::INDEX_BITS | index << Self::INDEX_SHIFT;
        self
    }
}
