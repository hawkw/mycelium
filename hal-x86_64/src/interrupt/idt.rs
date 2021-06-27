use crate::{cpu, segment};
use core::fmt;
use mycelium_util::bits;

#[repr(C)]
#[repr(align(16))]
pub struct Idt {
    pub(crate) descriptors: [Descriptor; Self::NUM_VECTORS],
}

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct Descriptor {
    offset_low: u16,
    pub segment: segment::Selector,
    ist_offset: u8,
    pub attrs: Attrs,
    offset_mid: u16,
    offset_hi: u32,
    _zero: u32,
}

#[derive(Eq, PartialEq, Copy, Clone)]
#[repr(transparent)]
pub struct Attrs(u8);

impl Descriptor {
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            segment: segment::Selector::null(),
            ist_offset: 0,
            attrs: Attrs::null(),
            offset_mid: 0,
            offset_hi: 0,
            _zero: 0,
        }
    }

    pub(crate) fn set_handler(&mut self, handler: *const ()) -> &mut Attrs {
        self.segment = segment::code_segment();
        let addr = handler as u64;
        self.offset_low = addr as u16;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_hi = (addr >> 32) as u32;
        self.attrs
            .set_present(true)
            .set_32_bit(true)
            .set_gate_kind(GateKind::Interrupt)
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum GateKind {
    Interrupt = 0b0000_0110,
    Trap = 0b0000_0111,
    Task = 0b0000_0101,
}

// === impl Idt ===

impl Idt {
    const NUM_VECTORS: usize = 256;

    /// Divide-by-zero interrupt (#D0)
    pub const DIVIDE_BY_ZERO: usize = 0;

    pub const DEBUG: usize = 1;

    /// Non-maskable interrupt.
    pub const NMI: usize = 2;

    pub const BREAKPOINT: usize = 3;

    pub const OVERFLOW: usize = 4;

    pub const BOUND_RANGE_EXCEEDED: usize = 5;

    pub const INVALID_OPCODE: usize = 6;

    /// A device not available exception
    pub const DEVICE_NOT_AVAILABLE: usize = 7;

    // TODO(eliza): can we enforce that this diverges?
    pub const DOUBLE_FAULT: usize = 8;

    /// On modern CPUs, this interrupt is reserved; this error fires a general
    /// protection fault instead.
    pub const COPROCESSOR_SEGMENT_OVERRUN: usize = 9;

    pub const INVALID_TSS: usize = 10;

    pub const SEGMENT_NOT_PRESENT: usize = 11;

    pub const STACK_SEGMENT_FAULT: usize = 12;

    pub const GENERAL_PROTECTION_FAULT: usize = 13;

    pub const PAGE_FAULT: usize = 14;

    pub const X87_FPU_EXCEPTION_PENDING: usize = 16;

    pub const ALIGNMENT_CHECK: usize = 17;

    pub const MACHINE_CHECK: usize = 18;

    pub const SIMD_FLOATING_POINT: usize = 19;

    pub const VIRTUALIZATION_EXCEPTION: usize = 20;

    pub const SECURITY_EXCEPTION: usize = 30;

    pub const fn new() -> Self {
        Self {
            descriptors: [Descriptor::null(); Self::NUM_VECTORS],
        }
    }

    pub(super) fn set_isr(&mut self, vector: usize, isr: *const ()) {
        let attrs = self.descriptors[vector].set_handler(isr);
        tracing::debug!(vector, isr = ?isr, ?attrs, "set isr");
    }

    pub fn load(&'static self) {
        let ptr = crate::cpu::DtablePtr::new(self);
        unsafe { asm!("lidt [{0}]", in(reg) &ptr) }
    }
}

impl fmt::Debug for Idt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.descriptors[..].iter()).finish()
    }
}

// === impl Descriptor ===

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Descriptor")
            .field("offset_low", &format_args!("{:#x}", self.offset_low))
            .field("segment", &self.segment)
            .field("ist_offset", &format_args!("{:#x}", self.ist_offset))
            .field("attrs", &self.attrs)
            .field("offset_mid", &format_args!("{:#x}", self.offset_mid))
            .field("offset_high", &format_args!("{:#x}", self.offset_hi))
            .finish()
    }
}

// === impl Attrs ===

impl Attrs {
    const KIND: bits::Pack8 = bits::Pack8::least_significant(3);
    const IS_32_BIT: bits::Pack8 = Self::KIND.next(1);
    const RING: bits::Pack8 = Self::IS_32_BIT.next(3);
    const PRESENT_BIT: bits::Pack8 = Self::RING.next(1);

    pub const fn null() -> Self {
        Self(0)
    }

    pub fn gate_kind(&self) -> GateKind {
        match Self::KIND.unpack(self.0) {
            0b0110 => GateKind::Interrupt,
            0b0111 => GateKind::Trap,
            0b0101 => GateKind::Task,
            bits => unreachable!("unexpected bit pattern {:#08b}", bits),
        }
    }

    pub fn is_32_bit(&self) -> bool {
        Self::IS_32_BIT.contained_in_any(self.0)
    }

    pub fn is_present(&self) -> bool {
        Self::PRESENT_BIT.contained_in_any(self.0)
    }

    pub fn ring(&self) -> cpu::Ring {
        cpu::Ring::from_u8(Self::RING.unpack(self.0))
    }

    pub fn set_gate_kind(&mut self, kind: GateKind) -> &mut Self {
        Self::KIND.pack_into_truncating(kind as u8, &mut self.0);
        self
    }

    pub fn set_32_bit(&mut self, is_32_bit: bool) -> &mut Self {
        if is_32_bit {
            Self::IS_32_BIT.set_all_in(&mut self.0);
        } else {
           Self::IS_32_BIT.unset_all_in(&mut self.0);
        }
        self
    }

    pub fn set_present(&mut self, present: bool) -> &mut Self {
        if present {
            Self::PRESENT_BIT.set_all_in(&mut self.0);
        } else {
           Self::PRESENT_BIT.unset_all_in(&mut self.0);
        }
        self
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        Self::RING.pack_into_truncating(ring as u8, &mut self.0);
        self
    }
}

impl fmt::Debug for Attrs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Attrs")
            .field("gate_kind", &self.gate_kind())
            .field("ring", &self.ring())
            .field("is_32_bit", &self.is_32_bit())
            .field("is_present", &self.is_present())
            .field("bits", &format_args!("{:b}", self))
            .finish()
    }
}

impl fmt::Binary for Attrs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Attrs")
            .field(&format_args!("{:#08b}", self.0))
            .finish()
    }
}

impl fmt::UpperHex for Attrs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Attrs")
            .field(&format_args!("{:#X}", self.0))
            .finish()
    }
}

impl fmt::LowerHex for Attrs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Attrs")
            .field(&format_args!("{:#x}", self.0))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idt_entry_is_correct_size() {
        use core::mem::size_of;
        assert_eq!(size_of::<Descriptor>(), 16);
    }

    #[test]
    fn attrs_pack_specs() {
        Attrs::KIND.assert_valid();
        Attrs::RING.assert_valid();
        Attrs::IS_32_BIT.assert_valid();
        Attrs::PRESENT_BIT.assert_valid();
    }

    #[test]
    fn idt_attrs_are_correct() {
        let mut present_32bit_interrupt = Attrs::null();
        present_32bit_interrupt
            .set_present(true)
            .set_32_bit(true)
            .set_gate_kind(GateKind::Interrupt);

        // expected bit pattern here is:
        // |   1|   0    0|   0|   1    1    1    0|
        // |   P|      DPL|   Z|          Gate Type|
        //
        // P: Present (1)
        // DPL: Descriptor Privilege Level (0 => ring 0)
        // Z: this bit is 0 for a 64-bit IDT. for a 32-bit IDT, this may be 1 for task gates.
        // Gate Type: 32-bit interrupt gate is 0b1110. that's just how it is.
        assert_eq!(
            present_32bit_interrupt.0 as u8, 0b1000_1110,
            "\n attrs: {:#?}",
            present_32bit_interrupt
        );
    }

    #[test]
    fn idt_entry_is_correct() {
        let mut idt_entry = Descriptor::null();
        idt_entry.set_handler(0x1234_8765_abcd_fdec as *const ());

        let idt_bytes = unsafe { core::mem::transmute::<&Descriptor, &[u8; 16]>(&idt_entry) };

        let expected_idt: [u8; 16] = [
            0xec, 0xfd, // offset bits 0..15 (little-endian? oh god)
            0x33, 0x00, // selector (.. wait, this is from the host...)
            0x00, // ist (no stack switching at the moment)
            0x8e, // type/attr bits, 0x8e for 32-bit ring-0 interrupt descriptor
            0xcd, 0xab, // bits 16..31 (still little-endian)
            0x65, 0x87, 0x34, 0x12, // and bits 32..63
            0x00, 0x00, 0x00, 0x00,
        ];

        assert_eq!(idt_bytes, &expected_idt, "\n entry: {:#?}", idt_entry);
    }
}
