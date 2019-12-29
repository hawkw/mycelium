use super::{Interrupt, Registers};
use crate::{cpu, segment};
use core::{marker::PhantomData, mem};
use hal_core::interrupt::Handlers;

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
    // _f: PhantomData<T>,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
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
            // _f: PhantomData,
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
        self.0 &= !Self::KIND_BITS;
        self.0 |= kind as u8;
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
        self.0 &= !Self::RING_BITS;
        self.0 |= ring;
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

impl core::fmt::Debug for Idt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.descriptors[..].iter()).finish()
    }
}

impl core::fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Descriptor")
            .field("offset_low", &format_args!("{:#x}", self.offset_low))
            .field("segment", &self.segment)
            .field("ist_offset", &format_args!("{:#x}", self.ist_offset))
            .field("attrs", &format_args!("Attrs({:#08b})", self.attrs.0))
            .field("offset_mid", &format_args!("{:#x}", self.offset_mid))
            .field("offset_high", &format_args!("{:#x}", self.offset_hi))
            .finish()
    }
}

// trait AsExternFnHack<I, O> {
//     extern "x86-interrupt" fn as_extern_fn(&self, inp: I) -> O;
// }

// impl<I> AsExternFnHack<I, ()> for fn(I) {
//     extern "x86-interrupt" fn as_extern_fn(&self, inp: I) -> O {
//         (self)(inp)
//     }
// }

// fn into_extern_fn(f: fn())

// use hal_core::interrupt::vectors;

// impl<'a>
//     hal_core::interrupt::RegisterHandler<
//         vectors::PageFault<super::PageFaultContext<'a>>,
//         super::PageFaultContext<'a>,
//     > for Idt
// {
//     fn register(
//         &mut self,
//         handler: fn(super::PageFaultContext<'a>),
//     ) -> Result<
//         (),
//         hal_core::interrupt::RegistrationError<vectors::PageFault<super::PageFaultContext<'a>>>,
//     > {
//         // self.descriptors[14]
//         //     .set_handler(
//         //         ((handler as AsExternFnHack<super::PageFaultContext<'a>, ()>).as_extern_fn_hack)
//         //             as *const (),
//         //     )
//         //     .set_gate_kind(GateKind::Trap);
//         unimplemented!();
//         Ok(())
//     }
// }

// impl<'a> hal_core::interrupt::RegisterHandler<vectors::Test<super::Context<'a>>, super::Context<'a>>
//     for Idt
// {
//     fn register(
//         &mut self,
//         handler: fn(super::Context<'a>),
//     ) -> Result<(), hal_core::interrupt::RegistrationError<vectors::Test<super::Context<'a>>>> {
//         fn null_handler(_: super::Context<'_>) {
//             unreachable!("bad")
//         }
//         static mut FNPTR: fn(super::Context<'_>) = null_handler;
//         extern "x86-interrupt" fn isr(cx: super::Context<'_>) {
//             panic!("asdf");
//             unsafe { (FNPTR)(cx) }
//         }
//         unsafe {
//             FNPTR = core::mem::transmute(handler);
//             (&mut self.descriptors[69]).set_handler(FNPTR as *const ());
//         }
//         Ok(())
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idt_entry_is_correct_size() {
        use core::mem::size_of;
        assert_eq!(size_of::<Descriptor>(), 16);
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
        assert_eq!(present_32bit_interrupt.0 as u8, 0b1000_1110);
    }

    #[test]
    fn idt_entry_is_correct() {
        use core::mem::size_of;

        let mut idt_entry = Descriptor::null();
        idt_entry.set_handler(0x12348765_abcdfdec as *const ());

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

        assert_eq!(idt_bytes, &expected_idt);
    }
}
