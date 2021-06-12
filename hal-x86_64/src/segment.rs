use crate::cpu;
use core::fmt;
use mycelium_util::bits::Pack16;

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Selector(u16);

/// A 64-bit mode user segment descriptor.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct UserDescriptor(u64);

/// A 64-bit mode descriptor for a system segment (such as an LDT or TSS
/// descriptor).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SystemDescriptor(u64, u64);

/// Returns the current code segment selector in `%cs`.
pub fn code_segment() -> Selector {
    let value: u16;
    unsafe { asm!("mov {0:x}, cs", out(reg) value) };
    Selector(value)
}

// === impl Selector ===

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

// === impl Descriptor ===

impl UserDescriptor {
    const DPL_SHIFT: u64 = 45;
    const DPL_BITS: u64 = 0b111 << Self::DPL_SHIFT;

    const ACCESSED: u64 = 1 << 40;
    const WRITABLE: u64 = 1 << 41;
    /// For code segments, sets the segment as “conforming”, influencing the
    /// privilege checks that occur on control transfers. For 32-bit data segments,
    /// sets the segment as "expand down". In 64-bit mode, ignored for data segments.
    const CONFORMING: u64 = 1 << 42;
    /// This flag must be set for code segments and unset for data segments.
    const EXECUTABLE: u64 = 1 << 43;
    /// This flag must be set for user segments (in contrast to system segments).
    const USER_SEGMENT: u64 = 1 << 44;
    /// The DPL for this descriptor is Ring 3. In 64-bit mode, ignored for data segments.
    const DPL_RING_3: u64 = 3 << 45;
    /// Must be set for any segment, causes a segment not present exception if not set.
    const PRESENT: u64 = 1 << 47;
    /// Available for use by the Operating System
    const AVAILABLE: u64 = 1 << 52;
    /// Must be set for 64-bit code segments, unset otherwise.
    const LONG_MODE: u64 = 1 << 53;
    /// Use 32-bit (as opposed to 16-bit) operands. If [`LONG_MODE`][Self::LONG_MODE] is set,
    /// this must be unset. In 64-bit mode, ignored for data segments.
    const DEFAULT_SIZE: u64 = 1 << 54;
    /// Limit field is scaled by 4096 bytes. In 64-bit mode, ignored for all segments.
    const GRANULARITY: u64 = 1 << 55;

    /// Bits `0..: u64 =15` of the limit field (ignored in 64-bit mode)
    const LIMIT_HIGH: u64 = 0xFFFF;
    /// Bits `16..: u64 =19` of the limit field (ignored in 64-bit mode)
    const LIMIT_16_19: u64 = 0xF << 48;
    /// Bits `0..: u64 =23` of the base field (ignored in 64-bit mode, except for fs and gs)
    const BASE_0_23: u64 = 0xFF_FFFF << 16;
    /// Bits `24..: u64 =31` of the base field (ignored in 64-bit mode, except for fs and gs)
    const BASE_24_31: u64 = 0xFF << 56;

    pub fn ring(&self) -> cpu::Ring {
        let dpl = self.0 & Self::DPL_BITS >> Self::DPL_SHIFT;
        cpu::Ring::from_u8(dpl as u8)
    }

    pub const fn with_ring(self, ring: cpu::Ring) -> Self {
        let ring_bits = (ring as u8 as u64) << Self::DPL_SHIFT;
        Self((self.0 & !Self::DPL_BITS) | ring_bits)
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
