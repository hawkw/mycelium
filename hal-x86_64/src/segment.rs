use crate::cpu;
use core::{arch::asm, fmt};
use mycelium_util::bits::Pack16;

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Selector(u16);

/// Returns the current code segment selector in `%cs`.
pub fn code_segment() -> Selector {
    let value: u16;
    unsafe { asm!("mov {0:x}, cs", out(reg) value) };
    Selector(value)
}

impl Selector {
    /// The first 2 least significant bits are the selector's priveliege ring.
    const RING: Pack16 = Pack16::least_significant(2);
    /// The next bit is set if this is an LDT segment selector.
    const LDT_BIT: Pack16 = Self::RING.next(1);
    /// The remaining bits are the index in the GDT/LDT.
    const INDEX: Pack16 = Self::LDT_BIT.next(5);

    pub const fn null() -> Self {
        Self(0)
    }

    pub const fn from_index(u: u16) -> Self {
        Self(Self::INDEX.pack_truncating(0, u))
    }

    pub const fn from_raw(u: u16) -> Self {
        Self(u)
    }

    pub fn ring(self) -> cpu::Ring {
        cpu::Ring::from_u8(Self::RING.unpack(self.0) as u8)
    }

    /// Returns true if this is an LDT segment selector.
    pub const fn is_ldt(&self) -> bool {
        Self::LDT_BIT.contained_in_any(self.0)
    }

    /// Returns true if this is a GDT segment selector.
    #[inline]
    pub const fn is_gdt(&self) -> bool {
        !self.is_ldt()
    }

    /// Returns the index into the LDT or GDT this selector refers to.
    pub const fn index(&self) -> u16 {
        Self::INDEX.unpack(self.0)
    }

    pub fn set_gdt(&mut self) -> &mut Self {
        Self::LDT_BIT.unset_all_in(&mut self.0);
        self
    }

    pub fn set_ldt(&mut self) -> &mut Self {
        Self::LDT_BIT.set_all_in(&mut self.0);
        self
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        Self::RING.pack_into(ring as u16, &mut self.0);
        self
    }

    pub fn set_index(&mut self, index: u16) -> &mut Self {
        Self::INDEX.pack_into(index, &mut self.0);
        self
    }

    pub fn bits(&self) -> u16 {
        self.0
    }
}

impl fmt::Debug for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("segment::Selector")
            .field("ring", &self.ring())
            .field("index", &self.index())
            .field("is_gdt", &self.is_gdt())
            .field("bits", &format_args!("{:#b}", self.0))
            .finish()
    }
}

impl fmt::UpperHex for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("segment::Selector")
            .field(&format_args!("{:#X}", self.0))
            .finish()
    }
}

impl fmt::LowerHex for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("segment::Selector")
            .field(&format_args!("{:#x}", self.0))
            .finish()
    }
}

impl fmt::Binary for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("segment::Selector")
            .field(&format_args!("{:#b}", self.0))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn segment_selector_is_correct_size() {
        assert_eq!(size_of::<Selector>(), 2);
    }

    #[test]
    fn selector_pack_specs_valid() {
        Selector::RING.assert_valid();
        Selector::LDT_BIT.assert_valid();
        Selector::INDEX.assert_valid();
    }
}
