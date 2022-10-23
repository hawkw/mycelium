use core::{arch::asm, convert::Infallible, fmt, mem};
use mycelium_util::bits;

pub mod entropy;
pub mod intrinsics;
mod msr;
pub use self::msr::Msr;

#[repr(transparent)]
pub struct Port {
    num: u16,
}

/// Describes which descriptor table ([GDT], [LDT], or [IDT]) a selector references.
///
/// [GDT]: crate::segment::Gdt
/// [LDT]: https://en.wikipedia.org/wiki/Global_Descriptor_Table#Local_Descriptor_Table
/// [IDT]: crate::interrupt::Idt
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DescriptorTable {
    /// The selector references a descriptor in the [Global Descriptor Table
    /// (GDT)][gdt].
    ///
    /// [gdt]: crate::segment::Gdt
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

/// An error indicating that a given CPU feature source is not supported on this
/// CPU.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FeatureNotSupported(&'static str);

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

/// Wait for an interrupt in a spin loop.
///
/// This is distinct from `core::hint::spin_loop`, as it is intended
/// specifically for waiting for an interrupt, rather than progress from another
/// thread. This should be called on each iteration of a loop that waits on a condition
/// set by an interrupt handler.
///
/// This function will execute one [`intrinsics::sti`] instruction to enable interrupts
/// followed by one [`intrinsics::hlt`] instruction to halt the CPU.
#[inline(always)]
pub fn wait_for_interrupt() {
    unsafe {
        intrinsics::sti();
        intrinsics::hlt();
    }
}

// === impl Port ===

impl fmt::Debug for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { num } = self;
        f.debug_struct("Port")
            .field("num", &format_args!("{num:#02x}"))
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
    /// Reading from a CPU port is unsafe.
    pub unsafe fn readl(&self) -> u32 {
        let result: u32;
        asm!("in eax, dx", in("dx") self.num, out("eax") result);
        result
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

impl bits::FromBits<u64> for Ring {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(u: u64) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(u as u8))
    }

    fn into_bits(self) -> u64 {
        self as u8 as u64
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
        unsafe {
            // safety: the `'static` lifetime ensures the pointed dtable is
            // never going away
            Self::new_unchecked(t)
        }
    }

    pub(crate) unsafe fn new_unchecked<T>(t: &T) -> Self {
        let limit = (mem::size_of::<T>() - 1) as u16;
        let base = t as *const _ as *const ();

        Self { limit, base }
    }
}

impl fmt::Debug for DtablePtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // avoid creating misaligned references by moving these i guess? idk why
        // rustc is okay with this but i'll trust it.
        let Self { limit, base } = *self;
        f.debug_struct("DtablePtr")
            .field("base", &format_args!("{base:0p}",))
            .field("limit", &limit)
            .finish()
    }
}

// === impl FeatureNotSupported ===

impl fmt::Display for FeatureNotSupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "this CPU does not support {}", self.0)
    }
}

impl FeatureNotSupported {
    pub fn feature_name(&self) -> &'static str {
        self.0
    }

    pub(crate) fn new(feature_name: &'static str) -> Self {
        Self(feature_name)
    }
}
