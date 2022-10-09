use super::device::{self, CardBusDetails, PciBridgeDetails, StandardDetails};
use core::ptr;

pub type StandardDevice = MemoryMappedDevice<StandardDetails>;
pub type PciBridgeDevice = MemoryMappedDevice<PciBridgeDetails>;
pub type CardBusDevice = MemoryMappedDevice<CardBusDetails>;

#[repr(C, packed)]
pub struct MemoryMappedDevice<T> {
    header: device::Header,
    details: T,
}

#[cfg(test)]
mod tests {
    // use super::Device;
    // use core::mem;

    // #[test]
    // fn device_is_256_bytes() {
    //     assert_eq!(mem::size_of::<Device>(), 256);
    // }
}
