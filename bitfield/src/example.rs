use crate::FromBits;
#[cfg(trace_macros)]
trace_macros!(true);
mod example_bitfield;
#[cfg(trace_macros)]
trace_macros!(false);
pub use self::example_bitfield::ExampleBitfield;

/// An example enum type implementing [`FromBits`].
#[repr(u8)]
#[derive(Debug)]
pub enum TestEnum {
    Foo = 0b00,
    Bar = 0b01,
    Baz = 0b10,
    Qux = 0b11,
}

impl FromBits<u64> for TestEnum {
    const BITS: u32 = 2;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        Ok(match bits as u8 {
            bits if bits == Self::Foo as u8 => Self::Foo,
            bits if bits == Self::Bar as u8 => Self::Bar,
            bits if bits == Self::Baz as u8 => Self::Baz,
            bits if bits == Self::Qux as u8 => Self::Qux,
            bits => unreachable!("all patterns are covered: {:#b}", bits),
        })
    }

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }
}

/// Another example enum type implementing [`FromBits`].
///
/// This one has a *fallible* [`FromBits::try_from_bits`] method, because some bit
/// patterns are not valid enum variants.
#[repr(u8)]
#[derive(Debug)]
pub enum AnotherTestEnum {
    Alice = 0b1000,
    Bob = 0b1100,
    Charlie = 0b1110,
}

impl FromBits<u64> for AnotherTestEnum {
    const BITS: u32 = 4;
    type Error = &'static str;

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        match bits as u8 {
            bits if bits == Self::Alice as u8 => Ok(Self::Alice),
            bits if bits == Self::Bob as u8 => Ok(Self::Bob),
            bits if bits == Self::Charlie as u8 => Ok(Self::Charlie),
            _ => Err("invalid bit pattern for `AnotherTestEnum`, expected \
                one of `0b1000`, `0b1100`, or `0b1110`"),
        }
    }

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }
}
