use core::{fmt, num::NonZeroU16, str::FromStr};
use hex::{FromHex, FromHexError};
use mycelium_bitfield::{bitfield, Pack32};

/// A PCI device address.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Address(AddressBits);

#[derive(Debug)]
pub struct ParseError {
    kind: ParseErrorKind,
}

#[derive(Debug)]

enum ParseErrorKind {
    ExpectedChar(char),
    NotANumber {
        reason: FromHexError,
        pos: &'static str,
    },
    InvalidNumber {
        num: u16,
        max: u16,
        name: &'static str,
    },
    Msg(&'static str),
}

bitfield! {
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
            write!(f, "{group:04X}:")?;
        }
        write!(
            f,
            "{:02X}:{:02X}.{}",
            self.bus(),
            self.device(),
            self.function()
        )
    }
}

impl fmt::LowerHex for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(group) = self.group() {
            write!(f, "{group:04x}:")?;
        }
        write!(
            f,
            "{:02x}:{:02x}.{}",
            self.bus(),
            self.device(),
            self.function()
        )
    }
}

impl fmt::Debug for Address {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({self:x})")
    }
}

impl FromStr for Address {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        fn parse_u8<T>(
            s: &str,
            mask: Pack32<T, AddressBits>,
            name: &'static str,
        ) -> Result<u8, ParseError> {
            let [num] = <[u8; 1]>::from_hex(s).map_err(ParseError::not_a_number(name))?;
            ParseError::validate_mask(num, mask, name)
        }

        let s = s.trim();
        let mut addr = Address::new();
        let mut split = s.split(':');
        let first = split.next().ok_or_else(|| ParseError::expected_char(':'))?;
        let second = split
            .next()
            .ok_or_else(|| ParseError::msg("expected a device number after ':'"))?;
        let (bus, dev_fn) = if let Some(third) = split.next() {
            // if there are two colons, the first part is the bus group.
            let bytes =
                <[u8; 2]>::from_hex(first).map_err(ParseError::not_a_number("bus group"))?;
            let group = u16::from_be_bytes(bytes);
            addr = addr.with_group(NonZeroU16::new(group));
            (second, third)
        } else {
            (first, second)
        };

        let bus = parse_u8(bus, AddressBits::BUS, "bus number")?;
        addr = addr.with_bus(bus);

        let mut dev_fn = dev_fn.split('.');
        let device = dev_fn
            .next()
            .ok_or_else(|| ParseError::msg("expected device number"))
            .and_then(|dev| parse_u8(dev, AddressBits::DEVICE, "device number"))?;
        addr = addr.with_device(device);

        let func = dev_fn
            .next()
            .map(|func| {
                // use `u8`'s `FromStr` impl rather than the `hex` crate for the
                // function number, as `hex` refuses to parse single-digit
                // strings as hex :<
                func.parse::<u8>()
                    .map_err(|_| {
                        ParseError::msg("function number is not a number in the range 0-8")
                    })
                    .and_then(|num| {
                        ParseError::validate_mask(num, AddressBits::FUNCTION, "function number")
                    })
            })
            .transpose()?;
        if let Some(func) = func {
            addr = addr.with_function(func);
        }
        Ok(addr)
    }
}

// === impl ParseError ===

impl ParseError {
    fn validate_mask<T>(
        num: u8,
        mask: Pack32<T, AddressBits>,
        name: &'static str,
    ) -> Result<u8, Self> {
        let max = mask.max_value() as u16;
        if num as u16 > max {
            return Err(Self {
                kind: ParseErrorKind::InvalidNumber {
                    num: num as u16,
                    max,
                    name,
                },
            });
        }

        Ok(num)
    }

    fn not_a_number(pos: &'static str) -> impl Fn(FromHexError) -> Self {
        move |reason| Self {
            kind: ParseErrorKind::NotANumber { reason, pos },
        }
    }
    fn expected_char(c: char) -> Self {
        Self {
            kind: ParseErrorKind::ExpectedChar(c),
        }
    }

    fn msg(msg: &'static str) -> Self {
        Self {
            kind: ParseErrorKind::Msg(msg),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ParseErrorKind::ExpectedChar(c) => write!(f, "expected a '{c}'")?,
            ParseErrorKind::InvalidNumber { num, max, name } => {
                write!(f, "{name} must be less than {max:#x} (got {num:#x})")?
            }
            ParseErrorKind::NotANumber { reason, pos } => {
                write!(f, "{pos} was not a valid hexadecimal number ({reason})")?
            }
            ParseErrorKind::Msg(msg) => f.write_str(msg)?,
        };
        f.write_str(
            ", PCI addresses must be in the format '(<BUS GROUP>:)<BUS>:<DEVICE>(.<FUNCTION>)'",
        )
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

    #[track_caller]
    fn test_parse(s: &str, expected: Address) {
        let addr = s.parse::<Address>().expect(s);
        assert_eq!(addr, expected);
    }

    #[test]
    fn parse_pci_addr_no_fn() {
        test_parse("00:02", Address::new().with_device(0x02));
        test_parse("0f:0f", Address::new().with_bus(0x000f).with_device(0x0f));
    }

    #[test]
    fn parse_pcie_addr_no_fn() {
        test_parse(
            "0000:0a:01",
            Address::new()
                .with_group(None)
                .with_bus(0x0a)
                .with_device(0x01),
        );
        test_parse(
            "1234:0f:0f",
            Address::new()
                .with_group(NonZeroU16::new(0x1234))
                .with_bus(0x0f)
                .with_device(0x0f),
        );
        test_parse(
            "ffff:0a:0b",
            Address::new()
                .with_group(NonZeroU16::new(0xffff))
                .with_bus(0x0a)
                .with_device(0x0b),
        );
    }

    #[test]
    fn parse_invalid() {
        println!("{}", "hello world".parse::<Address>().unwrap_err());
    }

    #[test]
    fn parse_pci_addr_with_fn() {
        test_parse("00:02.0", Address::new().with_device(0x02));
        test_parse("0f:0f.0", Address::new().with_bus(0x000f).with_device(0x0f));
        test_parse("00:02.1", Address::new().with_device(0x02).with_function(1));
        test_parse(
            "0f:0f.1",
            Address::new()
                .with_bus(0x000f)
                .with_device(0x0f)
                .with_function(1),
        );
    }

    #[test]
    fn parse_pcie_addr_with_fn() {
        test_parse(
            "0000:0a:01.0",
            Address::new()
                .with_group(None)
                .with_bus(0x0a)
                .with_device(0x01),
        );
        test_parse(
            "1234:0f:0f.0",
            Address::new()
                .with_group(NonZeroU16::new(0x1234))
                .with_bus(0x0f)
                .with_device(0x0f),
        );
        test_parse(
            "ffff:0a:0b.0",
            Address::new()
                .with_group(NonZeroU16::new(0xffff))
                .with_bus(0x0a)
                .with_device(0x0b),
        );

        test_parse(
            "0000:0a:01.1",
            Address::new()
                .with_group(None)
                .with_bus(0x0a)
                .with_device(0x01)
                .with_function(1),
        );
        test_parse(
            "1234:0f:0f.1",
            Address::new()
                .with_group(NonZeroU16::new(0x1234))
                .with_bus(0x0f)
                .with_device(0x0f)
                .with_function(1),
        );
        test_parse(
            "ffff:0a:0b.1",
            Address::new()
                .with_group(NonZeroU16::new(0xffff))
                .with_bus(0x0a)
                .with_device(0x0b)
                .with_function(1),
        );
    }
}
