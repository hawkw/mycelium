use core::{fmt, num::NonZeroU16};

/// A PCI device address.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Address(AddressBits);

mycelium_bitfield::bitfield! {
    #[derive(Eq, PartialEq, Ord, PartialOrd)]
    pub(crate) struct AddressBits<u32> {
        pub(crate) const FUNCTION = 3;
        pub(crate) const DEVICE = 5;
        pub(crate) const BUS: u8;
        pub(crate) const GROUP: u16;
    }
}

impl Address {
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self(AddressBits::new())
    }

    /// Returns the device's segment group, if it is a PCI Express device.
    ///
    /// PCI Express supports up to 65535 segment groups, each with 256 bus
    /// segments. Standard PCI does not support segment groups.
    #[inline]
    #[must_use]
    pub fn group(self) -> Option<NonZeroU16> {
        NonZeroU16::new(self.0.get(AddressBits::GROUP))
    }

    /// Returns the device's bus segment.
    ///
    /// PCI supports up to 256 bus segments.
    #[inline]
    #[must_use]
    pub fn bus(self) -> u8 {
        self.0.get(AddressBits::BUS)
    }

    /// Returns the device number within its bus segment.
    #[inline]
    #[must_use]
    pub fn device(self) -> u8 {
        self.0.get(AddressBits::DEVICE) as u8
    }

    /// Returns which function of the device this address refers to.
    ///
    /// A device may support up to 8 separate functions.
    #[inline]
    #[must_use]
    pub fn function(self) -> u8 {
        self.0.get(AddressBits::FUNCTION) as u8
    }

    #[inline]
    #[must_use]
    pub fn with_group(self, group: Option<NonZeroU16>) -> Self {
        let value = group.map(NonZeroU16::get).unwrap_or(0);
        Self(self.0.with(AddressBits::GROUP, value))
    }

    #[inline]
    #[must_use]
    pub fn with_bus(self, bus: u8) -> Self {
        Self(self.0.with(AddressBits::BUS, bus))
    }

    #[inline]
    #[must_use]
    pub fn with_device(self, device: u8) -> Self {
        Self(self.0.with(AddressBits::DEVICE, device as u32))
    }

    #[inline]
    #[must_use]
    pub fn with_function(self, function: u8) -> Self {
        Self(self.0.with(AddressBits::FUNCTION, function as u32))
    }

    pub(crate) fn bitfield(self) -> AddressBits {
        self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(self, f)
    }
}

impl fmt::UpperHex for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(group) = self.group() {
            write!(f, "{group:04X}-")?;
        }
        write!(
            f,
            "{:04X}:{:02X}.{}",
            self.bus(),
            self.device(),
            self.function()
        )
    }
}

impl fmt::LowerHex for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(group) = self.group() {
            write!(f, "{group:04x}-")?;
        }
        write!(
            f,
            "{:04x}:{:02x}.{}",
            self.bus(),
            self.device(),
            self.function()
        )
    }
}

impl fmt::Debug for Address {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({:x})", self)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use proptest::{prop_assert_eq, proptest};

    #[test]
    fn addrs_are_valid() {
        AddressBits::assert_valid();
    }

    proptest! {
        #[test]
        fn addr_roundtrips(bus in 0u8..255u8, device in 0u8..32u8, function in 0u8..8u8) {
            let addr = Address::new().with_bus(bus).with_device(device).with_function(function);

            prop_assert_eq!(addr.bus(), bus, "bus, addr: {}", addr);
            prop_assert_eq!(addr.device(), device, "device, addr: {}", addr);
            prop_assert_eq!(addr.function(), function, "function, addr: {}", addr);

        }
    }
}
