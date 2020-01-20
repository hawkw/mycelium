use core::fmt;
use core::mem;

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

    pub unsafe fn readb(&self) -> u8 {
        let result: u8;
        asm!("inb al, dx" : "={al}"(result) : "{dx}"(self.num) :: "volatile", "intel");
        result
    }

    pub unsafe fn writeb(&self, value: u8) {
        asm!("outb dx, al" :: "{dx}"(self.num), "{al}"(value) :: "volatile", "intel");
    }
    // TODO(ixi): anything wider than a byte lol
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
