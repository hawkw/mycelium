#![cfg_attr(not(test), no_std)]

pub mod class;
pub mod error;
pub mod express;

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
