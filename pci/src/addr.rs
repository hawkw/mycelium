use core::{convert::Infallible, fmt};
use mycelium_bitfield::{FromBits, Pack32};

use crate::error::{self, UnexpectedValue};

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Address(u32);

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Segment(u16);

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Bus(u8);

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Device(u8);

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Function(u8);

#[derive(Debug)]
pub struct FunctionIter(Option<Address>);

// === impl Address ===

impl Address {
    const FUNCTION: Pack32<Function> = Pack32::least_significant(3).typed();
    const DEVICE: Pack32<Device> = Self::FUNCTION.then();
    const BUS: Pack32<Bus> = Self::DEVICE.then();
    const SEGMENT: Pack32<Segment> = Self::BUS.then();

    pub fn new(segment: Segment, bus: Bus, device: Device, function: Function) -> Self {
        Self(
            Pack32::pack_in(0)
                .pack(bus, &Self::BUS)
                .pack(device, &Self::DEVICE)
                .pack(function, &Self::FUNCTION)
                .pack(segment, &Self::SEGMENT)
                .bits(),
        )
    }

    pub fn segment(self) -> Segment {
        Self::SEGMENT.unpack(self.0)
    }

    pub fn is_extended(self) -> bool {
        self.segment() != Segment(0)
    }

    pub fn bus(self) -> Bus {
        Self::BUS.unpack(self.0)
    }

    pub fn device(self) -> Device {
        Self::DEVICE.unpack(self.0)
    }

    pub fn function(self) -> Function {
        Self::FUNCTION.unpack(self.0)
    }

    fn next_function(self) -> Option<Self> {
        let Function(function) = self.function();
        if function == 7 {
            None
        } else {
            let bits = Self::FUNCTION.pack(Function(function + 1), self.0);
            Some(Self(bits))
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}:{}.{}",
            self.segment(),
            self.bus(),
            self.device(),
            self.function()
        )
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Address({}-{}:{}.{})",
            self.segment(),
            self.bus(),
            self.device(),
            self.function()
        )
    }
}

// === impl Bus ===

impl Bus {
    pub fn from_u8(u: u8) -> Self {
        Self(u)
    }
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bus({:#02x})", self.0)
    }
}

impl fmt::Display for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#02x}", self.0)
    }
}

impl FromBits<u8> for Bus {
    type Error = Infallible;
    const BITS: u32 = 8;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        Ok(Self(bits))
    }

    fn into_bits(self) -> u8 {
        self.0
    }
}

impl FromBits<u32> for Bus {
    type Error = Infallible;
    const BITS: u32 = 8;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self(bits as u8))
    }

    fn into_bits(self) -> u32 {
        self.0 as u32
    }
}

// === impl Segment ===

impl Segment {
    pub fn from_u16(u: u16) -> Self {
        Self(u)
    }
}

impl fmt::Debug for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Segment({:#02x})", self.0)
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#02x}", self.0)
    }
}

impl FromBits<u16> for Segment {
    type Error = Infallible;
    const BITS: u32 = 16;

    fn try_from_bits(bits: u16) -> Result<Self, Self::Error> {
        Ok(Self(bits))
    }

    fn into_bits(self) -> u16 {
        self.0
    }
}

impl FromBits<u32> for Segment {
    type Error = Infallible;
    const BITS: u32 = 16;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self(bits as u16))
    }

    fn into_bits(self) -> u32 {
        self.0 as u32
    }
}

// === impl Device ===

impl Device {
    pub fn try_from_u8(u: u8) -> Result<Self, UnexpectedValue<u8>> {
        if u > 31 {
            return Err(error::unexpected(u).named("a PCI device has up to 32 devices"));
        }

        Ok(Self(u))
    }
}

impl FromBits<u8> for Device {
    type Error = Infallible;
    const BITS: u32 = 5;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        Ok(Self(bits))
    }

    fn into_bits(self) -> u8 {
        self.0
    }
}

impl FromBits<u32> for Device {
    type Error = Infallible;
    const BITS: u32 = 5;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self(bits as u8))
    }

    fn into_bits(self) -> u32 {
        self.0 as u32
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Device({:#02x})", self.0)
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#02x}", self.0)
    }
}

// === impl Function ===

impl Function {
    pub fn try_from_u8(u: u8) -> Result<Self, UnexpectedValue<u8>> {
        if u > 8 {
            return Err(error::unexpected(u).named("a PCI device has up to 8 functions"));
        }

        Ok(Self(u))
    }
}

impl FromBits<u8> for Function {
    type Error = Infallible;
    const BITS: u32 = 3;

    fn try_from_bits(bits: u8) -> Result<Self, Self::Error> {
        Ok(Self(bits))
    }

    fn into_bits(self) -> u8 {
        self.0
    }
}

impl FromBits<u32> for Function {
    type Error = Infallible;
    const BITS: u32 = 3;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self(bits as u8))
    }

    fn into_bits(self) -> u32 {
        self.0 as u32
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Function({})", self.0)
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// === impl FunctionIter ===

impl Iterator for FunctionIter {
    type Item = Address;
    fn next(&mut self) -> Option<Address> {
        let addr = self.0?;
        self.0 = addr.next_function();
        Some(addr)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (8, Some(8))
    }
}
