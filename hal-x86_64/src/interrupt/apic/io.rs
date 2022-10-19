use mycelium_util::bits::{bitfield, FromBits};
use volatile::Volatile;

#[derive(Debug)]
pub struct IoApic<'mmio> {
    registers: Volatile<&'mmio mut MmioRegisters>,
}

bitfield! {
    pub struct RedirectionEntry<u64> {
        pub const VECTOR: u8;
        pub const DELIVERY: DeliveryMode;
        /// Destination mode.
        ///
        /// Physical (0) or logical (1). If this is physical mode, then bits
        /// 56-59 should contain an APIC ID. If this is logical mode, then those
        /// bits contain a set of processors.
        pub const DEST_MODE: DestinationMode;
        /// Set if this interrupt is going to be sent, but the APIC is busy. Read only.
        pub const QUEUED: bool;
        pub const POLARITY: PinPolarity;
        /// Used for level triggered interrupts only to show if a local APIC has
        /// received the interrupt (= 1), or has sent an EOI (= 0). Read only.
        pub const RECEIVED_LEVEL_TRIGGERED: bool;
        pub const TRIGGER: TriggerMode;
        pub const MASKED: bool;
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DestinationMode {
    Physical = 0,
    Logical = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PinPolarity {
    High = 0,
    Low = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TriggerMode {
    Edge = 0,
    Level = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryMode {
    /// Normal interrupt delivery.
    Normal = 0b000,
    /// Lowest priority.
    LowPriority = 0b001,
    /// System Management Interrupt (SMI).
    SystemManagement = 0b010,
    /// Non-Maskable Interrupt (NMI).
    NonMaskable = 0b100,
    /// "INIT" (what does this mean? i don't know!)
    Init = 0b101,
    /// External interrupt.
    External = 0b111,
}

/// Memory-mapped IOAPIC registers
#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct MmioRegisters {
    /// Selects the address to read/write from
    address: u32,
    /// The data to read/write
    data: u32,
}

// === impl IoApic ===

impl<'mmio> IoApic<'mmio> {
    const REDIRECTION_ENTRY_BASE: u32 = 0x10;

    #[must_use]
    pub fn entry(&mut self, irq: u32) -> RedirectionEntry {
        let register_low = Self::REDIRECTION_ENTRY_BASE + irq * 2;
        let low = self.read(register_low);
        let high = self.read(register_low + 1);
        RedirectionEntry::from_bits((high as u64) << 32 | low as u64)
    }

    pub fn set_entry(&mut self, irq: u32, entry: RedirectionEntry) {
        let register_low = Self::REDIRECTION_ENTRY_BASE + irq * 2;
        let bits = entry.bits();
        let low = bits as u32;
        let high = (bits >> 32) as u32;
        self.write(register_low, low);
        self.write(register_low + 1, high);
    }

    pub fn update_entry(
        &mut self,
        irq: u32,
        update: impl FnOnce(RedirectionEntry) -> RedirectionEntry,
    ) {
        let entry = self.entry(irq);
        let new_entry = update(entry);
        self.set_entry(irq, new_entry);
    }

    #[must_use]
    fn read(&mut self, offset: u32) -> u32 {
        self.set_offset(offset);
        self.registers.map_mut(|ioapic| &mut ioapic.data).read()
    }

    fn write(&mut self, offset: u32, value: u32) {
        self.set_offset(offset);
        self.registers
            .map_mut(|ioapic| &mut ioapic.data)
            .write(value)
    }

    fn set_offset(&mut self, offset: u32) {
        assert!(
            offset <= 0xff,
            "invalid IOAPIC register offset {:#x}",
            offset
        );
        self.registers
            .map_mut(|ioapic| &mut ioapic.address)
            .write(offset);
    }
}

// === impl DeliveryMode ===

impl Default for DeliveryMode {
    fn default() -> Self {
        Self::Normal
    }
}

impl FromBits<u64> for DeliveryMode {
    const BITS: u32 = 3;
    type Error = &'static str;

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        match bits {
            bits if bits as u8 == Self::Normal as u8 => Ok(Self::Normal),
            bits if bits as u8 == Self::LowPriority as u8 => Ok(Self::LowPriority),
            bits if bits as u8 == Self::SystemManagement as u8 => Ok(Self::SystemManagement),
            bits if bits as u8 == Self::NonMaskable as u8 => Ok(Self::NonMaskable),
            bits if bits as u8 == Self::Init as u8 => Ok(Self::Init),
            bits if bits as u8 == Self::External as u8 => Ok(Self::External),
            _ => Err(
                "IOAPIC delivery mode must be one of 0b000, 0b001, 0b010, 0b100, 0b101, or 0b111",
            ),
        }
    }
}

// === impl PinPolarity ===

impl FromBits<u64> for PinPolarity {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        Ok(match bits {
            0 => Self::High,
            1 => Self::Low,
            _ => unreachable!(),
        })
    }
}

// === impl DestinationMode ===

impl FromBits<u64> for DestinationMode {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        Ok(match bits {
            0 => Self::Physical,
            1 => Self::Logical,
            _ => unreachable!(),
        })
    }
}

// === impl TriggerMode ===

impl FromBits<u64> for TriggerMode {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        Ok(match bits {
            0 => Self::Edge,
            1 => Self::Level,
            _ => unreachable!(),
        })
    }
}
