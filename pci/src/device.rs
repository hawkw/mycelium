use crate::{class::Class, error, register};
use core::ptr;

#[derive(Debug)]
pub struct Device {
    pub header: Header,
    pub details: Kind,
}

#[derive(Debug)]
#[repr(C)]
pub struct Id {
    pub vendor_id: u16,
    pub device_id: u16,
}

mycelium_bitfield::bitfield! {
    #[derive(PartialEq, Eq)]
    pub struct HeaderTypeReg<u8> {
        /// Indicates the type of device and the layout of the header.
        pub const TYPE: HeaderType;
        const _RESERVED = 5;
        /// Indicates that this device has multiple functions.
        pub const MULTIFUNCTION: bool;
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum HeaderType {
    /// This is a standard PCI device.
    Standard = 0x00,
    /// This is a PCI-to-PCI bridge.
    PciBridge = 0x01,
    /// This is a PCI-to-CardBus bridge
    CardBusBridge = 0x02,
}

impl mycelium_bitfield::FromBits<u8> for HeaderType {
    type Error = error::UnexpectedValue<u8>;
    const BITS: u32 = 2;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        match bits {
            bits if bits == Self::Standard as u8 => Ok(Self::Standard),
            bits if bits == Self::PciBridge as u8 => Ok(Self::PciBridge),
            bits if bits == Self::CardBusBridge as u8 => Ok(Self::CardBusBridge),
            bits => Err(error::unexpected(bits)),
        }
    }

    fn into_bits(self) -> u8 {
        self as u8
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Header {
    pub id: Id,
    pub command: register::Command,
    pub status: register::Status,
    pub revision_id: u8,
    pub prog_if: u8,
    pub(crate) class: RawClasses,
    pub cache_line_size: u8,
    pub latency_timer: u8,
    pub header_type: HeaderTypeReg,
    pub bist: BistReg,
}

#[derive(Debug)]
pub enum Kind {
    Standard(StandardDetails),
    PciBridge(PciBridgeDetails),
    CardBus(CardBusDetails),
}

/// Much of the documentation for this struct's fields was copied from [the
/// OSDev Wiki][1].
///
/// [1]: https://wiki.osdev.org/Pci#Header_Type_0x0
#[derive(Debug)]
#[repr(C)]
pub struct StandardDetails {
    pub base_addrs: [u32; 6],
    /// Points to the Card Information Structure and is used by devices that
    /// share silicon between CardBus and PCI.
    pub cardbus_cis_ptr: u32,
    pub subsystem: SubsystemId,
    /// Expansion ROM base address.
    pub exp_rom_base_addr: u32,
    /// Points (i.e. an offset into this function's configuration space) to a
    /// linked list of new capabilities implemented by the device. Used if bit 4
    /// of the status register (Capabilities List bit) is set to 1. The bottom
    /// two bits are reserved and should be masked before the Pointer is used to
    /// access the Configuration Space.
    pub cap_ptr: u8,
    pub _res0: [u8; 7],
    /// Specifies which input of the system interrupt controllers the device's
    /// interrupt pin is connected to and is implemented by any device that
    /// makes use of an interrupt pin.
    ///
    /// For the x86 architectures, this register corresponds to the PIC IRQ
    /// numbers 0-15 (and not I/O APIC IRQ numbers)  and a value of 0xFF defines
    /// no connection.
    pub irq_line: u8,
    /// Specifies which interrupt pin the device uses.
    ///
    /// Where a value of `0x1` is `INTA#`, `0x2` is `INTB#`, `0x3` is `INTC#`,
    /// `0x4` is `INTD#`, and `0x0` means the
    /// device does not use an interrupt pin.
    pub irq_pin: u8,
    /// A read-only register that specifies the burst period length,
    /// in 1/4 microsecond units, that the device needs (assuming a 33 MHz clock
    /// rate).
    pub min_grant: u8,
    /// A read-only register that specifies how often the device needs access to
    /// the PCI bus (in 1/4 microsecond units).
    pub max_latency: u8,
}

#[derive(Debug)]
#[repr(C)]
pub struct PciBridgeDetails {
    base_addrs: [u32; 2],
    // WIP
}

#[derive(Debug)]
#[repr(C)]
pub struct CardBusDetails {
    // WIP
}

#[derive(Debug)]
#[repr(C)]
pub struct SubsystemId {
    pub(crate) vendor_id: u16,
    pub(crate) subsystem: u16,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct BistReg(pub(crate) u8);

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub(crate) struct RawClasses {
    pub(crate) subclass: u8,
    pub(crate) class: u8,
}

impl Header {
    pub fn header_type(&self) -> Result<HeaderType, error::UnexpectedValue<u8>> {
        self.header_type.try_get(HeaderTypeReg::TYPE)
    }

    pub fn is_multifunction(&self) -> bool {
        self.header_type.get(HeaderTypeReg::MULTIFUNCTION)
    }

    pub fn class(&self) -> Result<Class, error::UnexpectedValue<u8>> {
        (self.class, self.prog_if).try_into()
    }
}

impl BistReg {
    const CAPABLE_BIT: u8 = 0b1000_0000;
    const START_BIT: u8 = 0b0100_0000;
    const COMPLETION_MASK: u8 = 0b0000_0111;

    pub fn is_bist_capable(&self) -> bool {
        (self.0) & Self::CAPABLE_BIT != 0
    }

    pub fn start_bist(&mut self) {
        let val = (self.0) | Self::START_BIT;
        let ptr = (&mut self.0) as *mut u8;
        unsafe {
            ptr::write_volatile(ptr, val);
        }
    }

    pub fn completion_code(&self) -> u8 {
        (self.0) & Self::COMPLETION_MASK
    }
}
