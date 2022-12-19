use crate::{
    class::{Class, Classes, RawClasses, Subclass},
    error, register,
};
pub use bar::BaseAddress;
use mycelium_util::fmt;
pub use pci_ids::{Device as KnownId, Vendor};

mod bar;

/// A PCI device header.
///
/// This stores data common to all PCI devices, whether they are [standard PCI
/// devices](StandardDetails), [PCI-to-PCI bridges](PciBridgeDetails), or
/// [PCI-to-CardBus bridges](CardBusDetails).
///
/// The header has the following layout:
///
/// | Word | Bits 31-24    | Bits 23-16    | Bits 15-8      | Bits 7-0        |
/// |------|---------------|---------------|----------------|-----------------|
/// | 0x0  |[Device ID]    |               |[Vendor ID]     |                 |
/// | 0x1  |[`Status`]     |               |[`Command`]     |                 |
/// | 0x2  |[`Class`]      |[`Subclass`]   |[Prog IF]       |[Revision ID]    |
/// | 0x3  |[BIST] register|[`HeaderType`] |[Latency timer] |[Cache line size]|
///
/// Much of the documentation for this struct's fields was copied from [the
/// OSDev Wiki][wiki].
///
/// [Device ID]: Id
/// [Vendor ID]: Vendor
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
    /// The device's raw [vendor ID](Vendor) and [device ID](Id).
    ///
    /// Use the [`Header::id`] method to attempt to resolve the raw numeric IDs
    /// to a known vendor and device in the PCI IDs database.
    pub id: RawIds,
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
pub struct Device {
    pub header: Header,
    pub details: Kind,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct RawIds {
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

/// Represents a device's [vendor ID] and [device ID].
///
/// [vendor ID]: Vendor
/// [known ID]: KnownId
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Id {
    /// A known ID.
    ///
    /// This variant is returned if the device's vendor ID and device ID exist
    /// in the PCI ID database, and information such as a string representing
    /// the name of the vendor or device can be returned.
    Known(&'static KnownId),

    /// An unknown ID.
    ///
    /// This variant is returned if the device's vendor ID or device ID were not
    /// found in the PCI ID database.
    ///
    /// This is not necessarily an error, as it may indicate that the database
    /// has not been updated to include a newer device, the device is a
    /// prototype that has not yet been registered with PCI-SIG, or the device
    /// is not a "real" device but an emulated device provided by a virtual
    /// machine. An unknown device may still be used, provided that its
    /// [`Class`], [`Subclass`], and [prog IF] provide sufficient information to
    /// program that device.
    ///
    /// [prog IF]: Header#structfield.prog_if
    Unknown(RawIds),
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

/// Describes the type of device header.
///
/// This indicates whether the device is a [standard PCI
/// device](StandardDetails), a [PCI-to-PCI bridge](PciBridgeDetails), or a
/// [PCI-to-CardBus bridge](CardBusDetails).

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum HeaderType {
    /// This is a [standard PCI device](StandardDetails).
    Standard = 0x00,
    /// This is a [PCI-to-PCI bridge device](PciBridgeDetails).
    PciBridge = 0x01,
    /// This is a [PCI-to-CardBus bridge device](CardBusDetails).
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

/// Details describing specific types of device.
#[derive(Debug)]
pub enum Kind {
    /// A standard PCI device (not a bridge)
    Standard(StandardDetails),

    /// A PCI-to-PCI bridge device.
    PciBridge(PciBridgeDetails),

    /// A PCI-to-CardBus bridge device.
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
    /// numbers 0-15 (and not I/O APIC IRQ numbers) and a value of 0xFF defines
    /// no connection.
    pub irq_line: u8,
    /// Specifies which interrupt pin the device uses.
    ///
    /// A value of `0x1` is `INTA#`, `0x2` is `INTB#`, `0x3` is `INTC#`, `0x4`
    /// is `INTD#`, and `0x0` means the device does not use an interrupt pin.
    pub(crate) irq_pin: u8,
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

/// Specifies which interrupt pin a standard PCI device uses.
///
/// A value of `0x1` is `INTA#`, `0x2` is `INTB#`, `0x3` is `INTC#`, `0x4` is
/// `INTD#`, and `0x0` means the device does not use an interrupt pin.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum IrqPin {
    IntA = 0x1,
    IntB = 0x2,
    IntC = 0x3,
    IntD = 0x4,
}

impl Header {
    /// Returns the device's header type, or an error if the header type code is
    /// invalid.
    ///
    /// This value identifies the [device kind], and the layout of the rest of
    /// the device's PCI configuration space header.
    ///
    /// A device is one of the following:
    ///
    /// - A standard PCI device ([`StandardDetails`])
    /// - A PCI-to-PCI bridge ([`PciBridgeDetails`])
    /// - A PCI-to-CardBus bridge ([`CardBusDetails`])
    ///
    /// [device kind]: Kind
    #[inline]
    pub fn header_type(&self) -> Result<HeaderType, error::UnexpectedValue<u8>> {
        self.header_type
            .try_get(HeaderTypeReg::TYPE)
            .map_err(|e| e.named("header type"))
    }

    /// Returns `true` if this device has multiple functions.
    #[inline]
    #[must_use]
    pub fn is_multifunction(&self) -> bool {
        self.header_type.get(HeaderTypeReg::MULTIFUNCTION)
    }

    /// Returns the device's [`Id`].
    ///
    /// This will attempt to resolve the device's [vendor ID](Vendor) and
    /// [device ID](KnownId) in the PCI IDs database. If the device ID can be
    /// resolved, an [`Id::Known`] is returned. Otherwise, if the device's
    /// vendor ID or device ID does not exist in the PCI ID database, an
    /// [`Id::Unknown`] is returned.
    #[inline]
    #[must_use]
    pub fn id(&self) -> Id {
        self.id.resolve()
    }

    /// Returns the device's [`Classes`] (its [`Class`] and [`Subclass`]), or an
    /// error if this header's class or subclass code does not exist in the PCI
    /// ID database.
    #[inline]
    pub fn classes(&self) -> Result<Classes, error::UnexpectedValue<RawClasses>> {
        self.class.resolve()
    }

    /// Returns the device's [`Class`].
    ///
    /// The class indicates what general category of device (e.g. a display
    /// controller, network controller, etc) this header describes. The class
    /// value is used to determine the meaning of the [`Subclass`] code, which
    /// describes a subcategory of device within that class.
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(`[`Class`]`)` if the header's class code exists in the PCI ID
    ///   database.
    /// - [`Err`]`(`[`error::UnexpectedValue`]`)` if the header's class code was
    ///   not present in the PCI ID database. This generally indicates that the
    ///   device header was read incorrectly or the device is malfunctioning, as
    ///   all devices should have a known class code.
    #[inline]
    pub fn class(&self) -> Result<Class, error::UnexpectedValue<u8>> {
        self.class.resolve_class()
    }

    /// Returns the device's [`Subclass`].
    ///
    /// The subclass describes a more specific category of device within a
    /// [`Class`].
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(`[`Subclass`]`)` if the header's class and subclass codes
    ///   exist in the PCI ID database.
    /// - [`Err`]`(`[`error::UnexpectedValue`]`)` if the header's class or
    ///   subclass code was not present in the PCI ID database. This generally
    ///   indicates that the device header was read incorrectly or the device is
    ///   malfunctioning, as all devices should have known class and subclass
    ///   codes.
    #[inline]
    pub fn subclass(&self) -> Result<Subclass, error::UnexpectedValue<u8>> {
        self.class.resolve_subclass()
    }
}

impl StandardDetails {
    /// Returns this device's base address registers (BARs).
    ///
    /// # Returns
    ///
    /// - [`Ok`] containing an array of [`Option`]`<`[`BaseAddress`]`>` if the
    ///   BARs were successfully decoded.
    /// - [`Err`]`(`[`error::UnexpectedValue`]`)` if a BAR value is invalid.
    ///
    /// [`BaseAddress`]: bar::BaseAddress
    #[inline]
    pub fn base_addrs(&self) -> Result<[Option<bar::BaseAddress>; 6], error::UnexpectedValue<u32>> {
        bar::BaseAddress::decode_bars(&self.base_addrs)
    }

    /// Returns which IRQ pin this device uses.
    ///
    /// # Returns
    ///
    /// - [`Err`]`(`[`error::UnexpectedValue`]`)` if the value is not a valid IRQ
    ///   pin.
    /// - [`None`] if this device does not use an IRQ pin.
    /// - [`Some`]`(`[`IrqPin`]`)` if this device specifies a valid IRQ pin.
    pub fn irq_pin(&self) -> Result<Option<IrqPin>, error::UnexpectedValue<u8>> {
        match self.irq_pin {
            0x00 => Ok(None),
            0x01 => Ok(Some(IrqPin::IntA)),
            0x02 => Ok(Some(IrqPin::IntB)),
            0x03 => Ok(Some(IrqPin::IntC)),
            0x04 => Ok(Some(IrqPin::IntD)),
            bits => Err(error::unexpected(bits).named("IRQ pin")),
        }
    }
}

impl PciBridgeDetails {
    /// Returns this device's base address registers (BARs).
    pub fn base_addrs(&self) -> Result<[Option<bar::BaseAddress>; 2], error::UnexpectedValue<u32>> {
        bar::BaseAddress::decode_bars(&self.base_addrs)
    }
}

// === impl RawIds ===

impl RawIds {
    pub fn resolve(self) -> Id {
        match KnownId::from_vid_pid(self.vendor_id, self.device_id) {
            Some(known) => Id::Known(known),
            None => Id::Unknown(self),
        }
    }
}

impl fmt::LowerHex for RawIds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            device_id,
            vendor_id,
        } = self;
        // should the formatted ID be prefaced with a leading `0x`?
        let leading = if f.alternate() { "0x" } else { "" };
        write!(f, "{leading}{device_id:x}:{vendor_id:x}")
    }
}

// === impl Id ===

impl Id {
    #[inline]
    #[must_use]
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Self::Known(known) => Some(known.name()),
            _ => None,
        }
    }

    #[inline]
    #[must_use]
    pub fn vendor(&self) -> Option<&'static Vendor> {
        match self {
            Self::Known(known) => Some(known.vendor()),
            _ => None,
        }
    }

    #[inline]
    #[must_use]
    pub fn device_id(&self) -> u16 {
        match self {
            Self::Known(known) => known.id(),
            Self::Unknown(unknown) => unknown.device_id,
        }
    }

    #[inline]
    #[must_use]
    pub fn vendor_id(&self) -> u16 {
        match self {
            Self::Known(known) => known.vendor().id(),
            Self::Unknown(unknown) => unknown.vendor_id,
        }
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(known) => write!(f, "{} {}", known.vendor().name(), known.name()),

            Self::Unknown(RawIds {
                vendor_id,
                device_id,
            }) => write!(f, "{vendor_id:#x}:{device_id:x}"),
        }
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Id");
        match self {
            Self::Known(known) => s
                .field("vendor", known.vendor())
                .field("device", known)
                .finish(),

            Self::Unknown(RawIds {
                vendor_id,
                device_id,
            }) => s
                .field("vendor", &fmt::hex(vendor_id))
                .field("device", &fmt::hex(device_id))
                .finish(),
        }
    }
}
