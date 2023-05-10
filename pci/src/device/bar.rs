//! PCI Base Address Registers
use crate::error::{self, UnexpectedValue};
use mycelium_bitfield::{pack::Pack32, FromBits};
/// A PCI Base Address Register (BAR).
///
/// Base Address Registers (or BARs) can be used to hold memory addresses used
/// by the device, or offsets for port addresses. Typically, memory address BARs
/// need to be located in physical RAM, while I/O space BARs can reside at any
/// memory address (even beyond physical memory).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BaseAddress {
    Memory32 { prefetchable: bool, addr: u32 },
    Memory64 { prefetchable: bool, addr: u64 },
    Io(u32),
}

impl BaseAddress {
    const BAR_KIND: Pack32 = Pack32::least_significant(1);
    const MEM_TYPE: Pack32<MemoryBarType> = Self::BAR_KIND.then();
    const MEM_PREFETCHABLE: Pack32<bool> = Self::MEM_TYPE.then();
    const MEM_ADDR: Pack32 = Self::MEM_PREFETCHABLE.remaining();
    const MEM_MASK: u32 = Self::MEM_ADDR.raw_mask();

    // mask out the two low bits of an IO BAR to determine the address.
    const IO_MASK: u32 = !0b11;

    pub(crate) fn decode_bars<const BARS: usize>(
        bars: &[u32; BARS],
    ) -> Result<[Option<Self>; BARS], UnexpectedValue<u32>> {
        let mut decoded = [None; BARS];
        let mut curr_64_bit = None;
        for (i, &bits) in bars.iter().enumerate() {
            // if we have decoded half of a 64-bit BAR, this word is the upper
            // half.
            if let Some((prefetchable, low_bits)) = curr_64_bit.take() {
                let addr = ((bits as u64) << 32) | (low_bits as u64);
                // the previous index is a 64-bit BAR.
                decoded[i - 1] = Some(Self::Memory64 { prefetchable, addr });
                // this index is the upper half of the 64-bit BAR, so set it to
                // `None` in the output array.
                decoded[i] = None;
                continue;
            }

            // if the first bit is set, this is an IO BAR.
            if Self::BAR_KIND.unpack(bits) == 1 {
                decoded[i] = Some(Self::Io(bits & Self::IO_MASK));
                continue;
            }

            // okay, it's a memory BAR.
            let prefetchable = Self::MEM_PREFETCHABLE.unpack(bits);
            match Self::MEM_TYPE.try_unpack(bits)? {
                // this BAR has a 32-bit base address. decode it and continue.
                MemoryBarType::Base32 => {
                    decoded[i] = Some(Self::Memory32 {
                        prefetchable,
                        addr: bits & Self::MEM_MASK,
                    });
                }
                // this BAR has a 64-bit base address, so we'll use the next
                // 32-bit word as the high part of its address.
                MemoryBarType::Base64 => curr_64_bit = Some((prefetchable, bits & Self::MEM_MASK)),

                // 16-bit base addresses are not supported in PCI v3.0
                MemoryBarType::Legacy16 => {
                    return Err(
                        error::unexpected(bits).named("unsupported legacy 16-bit memory BAR")
                    );
                }
            }
        }

        Ok(decoded)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
enum MemoryBarType {
    /// 32-bit base address
    Base32 = 0x0,
    /// This is reserved in PCI v3.0. In earlier versions it indicated a 16-bit
    Legacy16 = 0x1,
    /// 64-bit base address
    Base64 = 0x2,
}

// === impl MemoryBarType ===

impl FromBits<u32> for MemoryBarType {
    type Error = UnexpectedValue<u32>;
    const BITS: u32 = 2;
    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        match bits as u8 {
            bits if bits == Self::Base32 as u8 => Ok(Self::Base32),
            bits if bits == Self::Base64 as u8 => Ok(Self::Base64),
            bits if bits == Self::Legacy16 as u8 => Ok(Self::Legacy16),
            _ => Err(crate::error::unexpected(bits).named("MemoryBarType")),
        }
    }

    fn into_bits(self) -> u32 {
        self as u8 as u32
    }
}
