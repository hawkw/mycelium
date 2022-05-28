use core::{arch::asm, convert::Infallible, fmt, mem};
use mycelium_util::bits;

pub mod intrinsics;

#[repr(transparent)]
pub struct Port {
    num: u16,
}

/// Describes which descriptor table ([GDT], [LDT], or [IDT]) a selector references.
///
/// [GDT]: https://en.wikipedia.org/wiki/Global_Descriptor_Table
/// [LDT]: https://en.wikipedia.org/wiki/Global_Descriptor_Table#Local_Descriptor_Table
/// [IDT]: crate::interrupt::Idt
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DescriptorTable {
    /// The selector references a descriptor in the [Global Descriptor Table
    /// (GDT)][gdt].
    ///
    /// [gdt]: https://en.wikipedia.org/wiki/Global_Descriptor_Table
    Gdt,

    /// The selector references an entry in a [Local Descriptor Table
    /// (LDT)][ldt]
    ///
    /// [ldt]: https://en.wikipedia.org/wiki/Global_Descriptor_Table#Local_Descriptor_Table
    Ldt,

    /// The selector references an interrupt handler in the [Interrupt
    /// Descriptor Table(IDT)][idt].
    ///
    /// [idt]: crate::interrupt::Idt
    Idt,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Ring {
    Ring0 = 0b00,
    Ring1 = 0b01,
    Ring2 = 0b10,
    Ring3 = 0b11,
}

#[repr(C, packed)]
pub(crate) struct DtablePtr {
    limit: u16,
    base: *const (),
}

/// Halt the CPU.
///
/// This disables interrupts and performs the `hlt` instruction in a loop,
/// forever.
///
/// # Notes
///
/// This halts the CPU.
#[inline(always)]
pub fn halt() -> ! {
    unsafe {
        // safety: this does exactly what it says on the tin lol
        intrinsics::cli();
        loop {
            intrinsics::hlt();
        }
    }
}

// === impl Port ===

impl fmt::Debug for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Port")
            .field("num", &format_args!("{:#02x}", self.num))
            .finish()
    }
}

impl Port {
    pub const fn at(address: u16) -> Self {
        Port { num: address }
    }

    /// # Safety
    ///
    /// Reading from a CPU port is unsafe.
    pub unsafe fn readb(&self) -> u8 {
        let result: u8;
        asm!("in al, dx", in("dx") self.num, out("al") result);
        result
    }

    /// # Safety
    ///
    /// Writing to a CPU port is unsafe.
    pub unsafe fn writeb(&self, value: u8) {
        asm!("out dx, al", in("dx") self.num, in("al") value)
    }

    /// # Safety
    ///
    /// Writing to a CPU port is unsafe.
    pub unsafe fn writel(&self, value: u32) {
        asm!("out dx, eax", in("dx") self.num, in("eax") value)
    }
}

// === impl DescriptorTable ===

impl fmt::Display for DescriptorTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gdt => f.pad("GDT"),
            Self::Ldt => f.pad("LDT"),
            Self::Idt => f.pad("IDT"),
        }
    }
}

impl bits::FromBits<u16> for DescriptorTable {
    type Error = Infallible;
    const BITS: u32 = 2;
    fn try_from_bits(bits: u16) -> Result<Self, Self::Error> {
        Ok(match bits {
            0b00 => Self::Gdt,
            0b01 => Self::Idt,
            0b10 => Self::Ldt,
            0b11 => Self::Idt,
            _ => unreachable!("only 2 bits should be unpacked!"),
        })
    }

    fn into_bits(self) -> u16 {
        todo!("eliza")
    }
}

// === impl Ring ===

impl Ring {
    pub fn from_u8(u: u8) -> Self {
        match u {
            0b00 => Ring::Ring0,
            0b01 => Ring::Ring1,
            0b10 => Ring::Ring2,
            0b11 => Ring::Ring3,
            bits => panic!("invalid ring {:#02b}", bits),
        }
    }
}

impl bits::FromBits<u16> for Ring {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(u: u16) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(u as u8))
    }

    fn into_bits(self) -> u16 {
        self as u8 as u16
    }
}

impl bits::FromBits<u8> for Ring {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(u: u8) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(u))
    }

    fn into_bits(self) -> u8 {
        self as u8
    }
}

// === impl DtablePtr ===

impl DtablePtr {
    pub(crate) fn new<T>(t: &'static T) -> Self {
        let limit = (mem::size_of::<T>() - 1) as u16;
        let base = t as *const _ as *const ();

        Self { limit, base }
    }
}
