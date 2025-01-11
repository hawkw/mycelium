// this module is mostly not yet implemented...
#![allow(dead_code)]
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

impl MemoryMappedDevice<'_> {
    pub fn header(&self) -> device::Header {
        self.header.read()
    }

    pub fn send_command(&mut self, command: register::Command) {
        // suppress unused warning
        let _ = command;
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
