use super::apic::IoApic;
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
    segment: segment::Selector,
    ist_offset: u8,
    attrs: Attrs,
    offset_mid: u16,
    offset_hi: u32,
    _zero: u32,
}

mycelium_util::bits::bitfield! {
    #[derive(Eq, PartialEq)]
    pub struct Attrs<u8> {
        pub const GATE_KIND: GateKind;
        pub const IS_32_BIT: bool;
        pub const RING: cpu::Ring;
        const _PAD = 1;
        pub const PRESENT: bool;
    }
}

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

    pub(crate) fn set_handler(&mut self, handler: *const ()) -> &mut Self {
        self.segment = segment::Selector::current_cs();
        let addr = handler as u64;
        self.offset_low = addr as u16;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_hi = (addr >> 32) as u32;
        self.attrs
            .set_present(true)
            .set_32_bit(true)
            .set_gate_kind(GateKind::Interrupt);
        self
    }

    /// Sets the descriptor's [Interrupt Stack Table][ist] offset.
    ///
    /// [ist]: https://en.wikipedia.org/wiki/Task_state_segment#Inner-level_stack_pointers
    pub(crate) fn set_ist_offset(&mut self, ist_offset: u8) -> &mut Self {
        self.ist_offset = ist_offset;
        self
    }

    /// Mutably borrows the descriptor's [attributes](Attrs).
    #[inline]
    #[must_use]
    pub fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum GateKind {
    Interrupt = 0b0000_0110,
    Trap = 0b0000_0111,
    Task = 0b0000_0101,
}

impl bits::FromBits<u8> for GateKind {
    const BITS: u32 = 3;
    type Error = &'static str;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        match bits {
            bits if bits == Self::Interrupt as u8 => Ok(Self::Interrupt),
            bits if bits == Self::Trap as u8 => Ok(Self::Trap),
            bits if bits == Self::Task as u8 => Ok(Self::Task),
            _ => Err("unknown GateKind pattern, expected one of [110, 111, or 101]"),
        }
    }

    fn into_bits(self) -> u8 {
        self as u8
    }
}

// === impl Idt ===

impl Idt {
    pub(super) const NUM_VECTORS: usize = 256;

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

    pub const X87_FPU_EXCEPTION: usize = 16;

    pub const ALIGNMENT_CHECK: usize = 17;

    pub const MACHINE_CHECK: usize = 18;

    pub const SIMD_FLOATING_POINT: usize = 19;

    pub const VIRTUALIZATION_EXCEPTION: usize = 20;

    pub const SECURITY_EXCEPTION: usize = 30;

    /// Chosen by fair die roll, guaranteed to be random.
    pub const DOUBLE_FAULT_IST_OFFSET: usize = 4;

    pub const PIC_PIT_TIMER: usize = Self::PIC_BIG_START;
    pub const PIC_PS2_KEYBOARD: usize = Self::PIC_BIG_START + 1;

    pub(super) const LOCAL_APIC_TIMER: usize = (Self::NUM_VECTORS - 2);
    pub(super) const LOCAL_APIC_SPURIOUS: usize = (Self::NUM_VECTORS - 1);
    pub(super) const PIC_BIG_START: usize = 0x20;
    pub(super) const PIC_LITTLE_START: usize = 0x28;
    // put the IOAPIC right after the PICs
    pub(super) const IOAPIC_START: usize = 0x30;
    pub(super) const IOAPIC_PIT_TIMER: usize = Self::IOAPIC_START + IoApic::PIT_TIMER_IRQ as usize;
    pub(super) const IOAPIC_PS2_KEYBOARD: usize =
        Self::IOAPIC_START + IoApic::PS2_KEYBOARD_IRQ as usize;

    pub const fn new() -> Self {
        Self {
            descriptors: [Descriptor::null(); Self::NUM_VECTORS],
        }
    }

    pub(super) fn set_isr(&mut self, vector: usize, isr: *const ()) {
        let descr = self.descriptors[vector].set_handler(isr);
        if vector == Self::DOUBLE_FAULT {
            descr.set_ist_offset(Self::DOUBLE_FAULT_IST_OFFSET as u8);
        }
        tracing::debug!(vector, ?isr, ?descr, "set isr");
    }

    pub fn load(&'static self) {
        unsafe {
            // Safety: the `'static` bound ensures the IDT isn't going away
            // unless you did something really evil.
            self.load_raw()
        }
    }

    /// # Safety
    ///
    /// The referenced IDT must be valid for the `'static` lifetime.
    pub unsafe fn load_raw(&self) {
        let ptr = cpu::DtablePtr::new_unchecked(self);
        tracing::debug!(?ptr, "loading IDT");
        cpu::intrinsics::lidt(ptr);
        tracing::debug!("IDT loaded!");
    }
}

impl fmt::Debug for Idt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self { descriptors } = self;
        f.debug_list().entries(descriptors[..].iter()).finish()
    }
}

// === impl Descriptor ===

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self {
            offset_low,
            segment,
            ist_offset,
            attrs,
            offset_mid,
            offset_hi,
            _zero: _,
        } = self;
        f.debug_struct("Descriptor")
            .field("offset_low", &format_args!("{offset_low:#x}"))
            .field("segment", segment)
            .field("ist_offset", &format_args!("{ist_offset:#x}"))
            .field("attrs", attrs)
            .field("offset_mid", &format_args!("{offset_mid:#x}"))
            .field("offset_high", &format_args!("{offset_hi:#x}"))
            .finish()
    }
}

// === impl Attrs ===

impl Attrs {
    pub const fn null() -> Self {
        Self(0)
    }

    pub fn gate_kind(&self) -> GateKind {
        self.get(Self::GATE_KIND)
    }

    pub fn is_32_bit(&self) -> bool {
        self.get(Self::IS_32_BIT)
    }

    pub fn is_present(&self) -> bool {
        self.get(Self::PRESENT)
    }

    pub fn ring(&self) -> cpu::Ring {
        self.get(Self::RING)
    }

    pub fn set_gate_kind(&mut self, kind: GateKind) -> &mut Self {
        self.set(Self::GATE_KIND, kind)
    }

    pub fn set_32_bit(&mut self, is_32_bit: bool) -> &mut Self {
        self.set(Self::IS_32_BIT, is_32_bit)
    }

    pub fn set_present(&mut self, present: bool) -> &mut Self {
        self.set(Self::PRESENT, present)
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        self.set(Self::RING, ring)
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
        Attrs::assert_valid()
    }

    #[test]
    fn idt_attrs_are_correct() {
        let mut present_32bit_interrupt = Attrs::null();
        present_32bit_interrupt
            .set_present(true)
            .set_32_bit(true)
            .set_gate_kind(GateKind::Interrupt);
        println!("{present_32bit_interrupt}");
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
