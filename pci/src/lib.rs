#![cfg_attr(not(test), no_std)]

pub mod class;
pub mod error;
pub mod express;

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct HeaderType(u8);

impl HeaderType {
    const MF_BIT: u8 = 0b1000_0000;
    const TYPE_STANDARD: u8 = 0x00;
    const TYPE_PCI_BRIDGE: u8 = 0x01;
    const TYPE_CARDBUS_BRIDGE: u8 = 0x02;
    const TYPE_BITS: u8 = Self::TYPE_PCI_BRIDGE | Self::TYPE_CARDBUS_BRIDGE;
    const NEVER_SET: u8 = !(Self::MF_BIT & Self::TYPE_BITS);

    pub fn is_standard(self) -> bool {
        (self.0 & Self::TYPE_BITS) == Self::TYPE_STANDARD
    }

    pub fn is_pci_bridge(self) -> bool {
        (self.0 & Self::TYPE_BITS) == Self::TYPE_PCI_BRIDGE
    }

    pub fn is_cardbus_bridge(self) -> bool {
        (self.0 & Self::TYPE_BITS) == Self::TYPE_CARDBUS_BRIDGE
    }

    pub fn is_multifunction(self) -> bool {
        self.0 & Self::MF_BIT != 0
    }
}

impl core::convert::TryFrom<u8> for HeaderType {
    type Error = error::UnexpectedValue<u8>;
    fn try_from(u: u8) -> Result<Self, Self::Error> {
        if u & Self::NEVER_SET != 0 {
            return Err(error::unexpected(u));
        }
        Ok(Self(u))
    }
}
