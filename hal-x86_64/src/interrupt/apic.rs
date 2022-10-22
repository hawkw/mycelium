//! Advanced Programmable Interrupt Controller (APIC).
pub mod io;
pub mod local;
pub use io::IoApic;
pub use local::LocalApic;

use mycelium_util::bits::FromBits;
use raw_cpuid::CpuId;

pub fn is_supported() -> bool {
    CpuId::new()
        .get_feature_info()
        .map(|features| features.has_apic())
        .unwrap_or(false)
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

// === impl PinPolarity ===

impl PinPolarity {
    #[inline(always)]
    #[must_use]
    fn from_u8(bits: u8) -> Self {
        match bits {
            0 => Self::High,
            1 => Self::Low,
            bits => unreachable!(
                "APIC PinPolarity should be a single bit, but got {:#b}",
                bits
            ),
        }
    }
}

impl FromBits<u64> for PinPolarity {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(bits as u8))
    }
}

impl FromBits<u32> for PinPolarity {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u32 {
        self as u8 as u32
    }

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(bits as u8))
    }
}
// === impl TriggerMode ===

impl TriggerMode {
    #[inline(always)]
    #[must_use]
    fn from_u8(bits: u8) -> Self {
        match bits {
            0 => Self::Edge,
            1 => Self::Level,
            _ => unreachable!(
                "APIC TriggerMode should be a single bit, but got {:#b}",
                bits
            ),
        }
    }
}

impl FromBits<u64> for TriggerMode {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u64 {
        self as u8 as u64
    }

    fn try_from_bits(bits: u64) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(bits as u8))
    }
}

impl FromBits<u32> for TriggerMode {
    const BITS: u32 = 1;
    type Error = core::convert::Infallible;

    fn into_bits(self) -> u32 {
        self as u8 as u32
    }

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self::from_u8(bits as u8))
    }
}
