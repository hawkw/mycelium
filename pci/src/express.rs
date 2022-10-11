use crate::{
    device::{self, CardBusDetails, PciBridgeDetails, StandardDetails},
    register,
};
use volatile::Volatile;

pub struct MemoryMappedDevice<'device> {
    header: Volatile<&'device mut device::Header>,
    details: MmKind<'device>,
}

enum MmKind<'device> {
    Standard(Volatile<&'device mut StandardDetails>),
    CardBusBridge(Volatile<&'device mut CardBusDetails>),
    PciBridge(Volatile<&'device mut PciBridgeDetails>),
}

impl<'device> MemoryMappedDevice<'device> {
    pub fn header(&self) -> device::Header {
        self.header.read()
    }

    pub fn send_command(&mut self, command: register::Command) {
        todo!()
    }
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
