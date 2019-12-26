use crate::{cpu, segment};
use core::marker::PhantomData;

#[repr(C)]
#[repr(align(16))]
pub struct Idt {
    descriptors: [Descriptor; Self::NUM_VECTORS],
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Descriptor<T = ()> {
    offset_low: u16,
    pub segment: segment::Selector,
    ist_offset: u16,
    pub attrs: Attrs,
    offset_mid: u16,
    offset_hi: u32,
    _zero: u32,
    _f: PhantomData<Isr<T>>,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(transparent)]
pub struct Attrs(u8);

impl<T> Descriptor<T> {
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            segment: segment::Selector::null(),
            ist_offset: 0,
            attrs: Attrs::null(),
            offset_mid: 0,
            offset_hi: 0,
            _zero: 0,
            _f: PhantomData,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum GateKind {
    Interrupt = 0b0001_0110,
    Trap = 0b0000_0111,
    Task = 0b0000_0101,
}

impl Attrs {
    const IS_32_BIT: u8 = 0b1000;
    const KIND_BITS: u8 = GateKind::Interrupt as u8 | GateKind::Trap as u8 | GateKind::Task as u8;
    const PRESENT_BIT: u8 = 0b1000_0000;
    const RING_BITS: u8 = 0b0111_0000;
    const RING_SHIFT: u8 = Self::RING_BITS.trailing_zeros() as u8;

    pub const fn null() -> Self {
        Self(0)
    }

    pub fn gate_kind(&self) -> GateKind {
        match self.0 & Self::KIND_BITS {
            0b0110 => GateKind::Interrupt,
            0b0111 => GateKind::Trap,
            0b0101 => GateKind::Task,
            bits => unreachable!("unexpected bit pattern {:#08b}", bits),
        }
    }

    pub fn is_32_bit(&self) -> bool {
        self.0 & Self::IS_32_BIT != 0
    }

    pub fn is_present(&self) -> bool {
        self.0 & Self::PRESENT_BIT == Self::PRESENT_BIT
    }

    pub fn ring(&self) -> cpu::Ring {
        cpu::Ring::from_u8(self.0 & Self::RING_BITS >> Self::RING_SHIFT)
    }

    pub fn set_gate_kind(&mut self, kind: GateKind) -> &mut Self {
        self.0 &= Self::IS_32_BIT | kind as u8;
        self
    }

    pub fn set_32_bit(&mut self, is_32_bit: bool) -> &mut Self {
        if is_32_bit {
            self.0 |= Self::IS_32_BIT;
        } else {
            self.0 &= !Self::IS_32_BIT;
        }
        self
    }

    pub fn set_present(&mut self, present: bool) -> &mut Self {
        if present {
            self.0 |= Self::PRESENT_BIT;
        } else {
            self.0 &= !Self::PRESENT_BIT;
        }
        self
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        let ring = (ring as u8) << Self::RING_SHIFT;
        self.0 &= !Self::RING_BITS | ring;
        self
    }
}

impl Idt {
    const NUM_VECTORS: usize = 256;

    pub const fn new() -> Self {
        Self {
            descriptors: [Descriptor::null(); Self::NUM_VECTORS],
        }
    }

    pub fn load(&'static self) {
        let ptr = crate::cpu::DtablePtr::new(self);
        unsafe { asm!("lidt ($0)" :: "r" (&ptr) : "memory") }
    }
}
