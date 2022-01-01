use core::{arch::asm, fmt, mem};

pub mod intrinsics;

#[repr(transparent)]
pub struct Port {
    num: u16,
}

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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Ring {
    Ring0 = 0b00,
    Ring1 = 0b01,
    Ring2 = 0b10,
    Ring3 = 0b11,
}

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

#[repr(C, packed)]
pub(crate) struct DtablePtr {
    limit: u16,
    base: *const (),
}

impl DtablePtr {
    pub(crate) fn new<T>(t: &'static T) -> Self {
        let limit = (mem::size_of::<T>() - 1) as u16;
        let base = t as *const _ as *const ();

        Self { limit, base }
    }
}

/// Halt the CPU.
///
/// This disables interrupts and performs the `hlt` instruction in a loop,
/// forever.
///
/// # Safety
///
/// This halts the CPU.
#[inline(always)]
pub unsafe fn halt() -> ! {
    intrinsics::cli();
    loop {
        intrinsics::hlt();
    }
}
