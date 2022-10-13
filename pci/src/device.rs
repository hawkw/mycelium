use crate::{class::Class, error, register};
pub use bar::BaseAddress;
mod bar;
#[derive(Debug)]
pub struct Device {
    pub header: Header,
    pub details: Kind,
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Id {
    /// Identifies the manufacturer of the device.
    ///
    /// PCI vendor IDs are allocated by PCI-SIG to ensure uniqueness; a complete
    /// list is available [here]. Vendor ID `0xFFFF` is reserved to indicate
    /// that a device is not present.
    ///
    /// [here]: https://pcisig.com/membership/member-companies
    pub vendor_id: u16,
    /// Identifies the specific device.
    ///
    /// Device IDs are allocated by the device's vendor.
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

/// A PCI device header.
///
/// This stores data common to all PCI devices, whether they are [standard PCI
/// devices](StandardDetails), [PCI-to-PCI bridges](PciBridgeDetails), or
/// [PCI-to-CardBus bridges](CardBusDetails).
///
/// The header has the following layout:
///
/// | Bits 31-24      | Bits 23-16      | Bits 15-8       | Bits 7-0        |
/// |-----------------|-----------------|-----------------|-----------------|
/// | [Device ID]     |                 | [Vendor ID]     |                 |
/// | [`Status`]      |                 | [`Command`]     |                 |
/// | [`Class`] code  | Subclass code   | [Prog IF]       | [Revision ID]   |
/// | [BIST] register | [`HeaderType`]  | [Latency timer] |[Cache line size]|
///
/// Much of the documentation for this struct's fields was copied from [the
/// OSDev Wiki][wiki].
///
/// [Device ID]: Id#structfield.device_id
/// [Vendor ID]: Id#structfield.vendor_id
/// [Prog IF]: #structfield.prog_if
/// [Revision ID]: #structfield.revision_id
/// [Latency timer]: #structfield.latency_timer
/// [Cache line size]: #structfield.cache_line_size
/// [`Status`]: register::Status
/// [`Command`]: register::Command
/// [BIST]: register::Bist
/// [wiki]: https://wiki.osdev.org/Pci#Common_Header_Fields
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Header {
    /// The device's vendor ID and device ID.
    pub id: Id,
    /// The device's [`Command`] register.
    ///
    /// This register provides control over a device's ability to generate and
    /// respond to PCI cycles. When a 0 is written to this register, the device
    /// is disconnected from the PCI bus. Other values may be written to this
    /// register to send other commands, depending on the device.
    ///
    /// [`Command`]: register::Command
    pub command: register::Command,
    /// The device's [`Status`] register.
    ///
    /// This register can be read from to access status information about PCI
    /// events.
    ///
    /// [`Status`]: register::Command
    pub status: register::Status,
    /// Specifies a revision identifier for a particular device.
    ///
    /// Revision IDs are allocated by the device's vendor.
    pub revision_id: u8,
    /// Programming interface.
    ///
    /// A read-only register that specifies a register-level programming
    /// interface the device has, if it has any at all.
    ///
    /// This is often used alongside the device's class and subclass to
    /// determine how to interact with the device.
    pub prog_if: u8,
    /// The device's class and subclass.
    ///
    /// See the [`class`](crate::class) module for details.
    pub(crate) class: RawClasses,
    /// Specifies the system cache line size in 32-bit units.
    ///
    /// A device can limit the number of cacheline sizes it can support, if a
    /// unsupported value is written to this field, the device will behave as if
    /// a value of 0 was written.
    pub cache_line_size: u8,
    /// Specifies the latency timer in units of PCI bus clocks.
    pub latency_timer: u8,
    /// Identifies the [device kind] and the layout of the rest of the
    /// device's PCI configuration space header.
    ///
    /// A device is one of the following:
    ///
    /// - A standard PCI device ([`StandardDetails`])
    /// - A PCI-to-PCI bridge ([`PciBridgeDetails`])
    /// - A PCI-to-CardBus bridge ([`CardBusDetails`])
    ///
    /// [device kind]: Kind
    pub header_type: HeaderTypeReg,
    /// A read-write register for running the device's Built-In Self Test
    /// (BIST).
    pub bist: register::Bist,
}

#[derive(Debug)]
pub enum Kind {
    Standard(StandardDetails),
    PciBridge(PciBridgeDetails),
    CardBus(CardBusDetails),
}

/// A header describing a standard PCI device (not a bridge).
///
/// Much of the documentation for this struct's fields was copied from [the
/// OSDev Wiki][1].
///
/// [1]: https://wiki.osdev.org/Pci#Header_Type_0x0
#[derive(Debug)]
#[repr(C)]
pub struct StandardDetails {
    pub(crate) base_addrs: [u32; 6],
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
    pub(crate) _res0: [u8; 7],
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
    pub(crate) base_addrs: [u32; 2],
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

impl StandardDetails {
    /// Returns this device's base address registers (BARs).
    pub fn base_addrs(&self) -> Result<[Option<bar::BaseAddress>; 6], error::UnexpectedValue<u32>> {
        bar::BaseAddress::decode_bars(&self.base_addrs)
    }
}

impl PciBridgeDetails {
    /// Returns this device's base address registers (BARs).
    pub fn base_addrs(&self) -> Result<[Option<bar::BaseAddress>; 2], error::UnexpectedValue<u32>> {
        bar::BaseAddress::decode_bars(&self.base_addrs)
    }
}
