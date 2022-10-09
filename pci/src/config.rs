//! PCI Bus Configuration Space
//!
//! See https://wiki.osdev.org/Pci#Configuration_Space_Access_Mechanism_.231
use crate::{device, Device};
use core::fmt;
use hal_x86_64::cpu::Port;

#[derive(Copy, Clone, Debug)]
pub struct ConfigAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Debug)]
pub struct ConfigReg {
    data_port: Port,
    addr_port: Port,
    addr: ConfigAddressBits,
}

pub fn enumerate_bus(bus: u8) -> impl Iterator<Item = (ConfigAddress, Device)> {
    let addrs = (0..32u8).map(move |device| ConfigAddress {
        device,
        bus,
        function: 0,
    });

    addrs
        .filter_map(|addr| Some((addr, ConfigReg::new(addr).read_device()?)))
        .flat_map(|(addr, device)| {
            // if the device is multifunction, enumerate up to 8 additional functions.
            let function_nums = device
                .header
                .is_multifunction()
                .then_some(1..8)
                .into_iter()
                .flatten();
            let functions = function_nums.filter_map(move |function| {
                let addr = ConfigAddress { function, ..addr };
                Some((addr, ConfigReg::new(addr).read_device()?))
            });
            core::iter::once((addr, device)).chain(functions)
        })
}

/// Enumerate all PCI buses
pub fn enumerate_all() -> impl Iterator<Item = (ConfigAddress, Device)> {
    (0..=255u8).flat_map(enumerate_bus)
}

mycelium_bitfield::bitfield! {
    /// The PCI bus `CONFIG_ADDRESS` register (port `0xCF8`).
    ///
    /// |Bit 31    |Bits 30-24|Bits 23-16|Bits 15-11   |Bits 10-8      |Bits 7-0       |
    /// |:---------|----------|----------|:------------|:--------------|:--------------|
    /// |Enable Bit|Reserved  |Bus Number|Device Number|Function Number|Register Offset|
    struct ConfigAddressBits<u32> {
        const REGISTER_OFFSET: u8;
        const FUNCTION = 3;
        const DEVICE = 5;
        const BUS: u8;
        const _RESERVED = 7;
        const ENABLE: bool;
    }
}

impl ConfigReg {
    fn new(addr: ConfigAddress) -> Self {
        Self {
            data_port: Port::at(DATA_PORT),
            addr_port: Port::at(ADDRESS_PORT),
            addr: ConfigAddressBits::from_address(addr),
        }
    }

    fn read_offset(&self, offset: u8) -> u32 {
        let addr = self.addr.with(ConfigAddressBits::REGISTER_OFFSET, offset);
        unsafe {
            self.addr_port.writel(addr.bits());
            self.data_port.readl()
        }
    }

    pub fn read_device(&self) -> Option<device::Device> {
        let header = self.read_header()?;
        let details = match header.header_type() {
            Ok(device::HeaderType::Standard) => {
                let base_addrs = [
                    self.read_offset(0x10),
                    self.read_offset(0x14),
                    self.read_offset(0x18),
                    self.read_offset(0x1C),
                    self.read_offset(0x20),
                    self.read_offset(0x24),
                ];
                // tfw no cardbus trans pointer T___T
                let cardbus_cis_ptr = self.read_offset(0x28);
                let subsystem = {
                    let word = self.read_offset(0x2C);
                    device::SubsystemId {
                        // XXX(eliza): if the subsystem vendor ID is 0xFFFF,
                        // does this mean "no subsystem"?
                        vendor_id: (word & 0xffff) as u16,
                        subsystem: (word >> 16) as u16,
                    }
                };
                let exp_rom_base_addr = self.read_offset(0x30);
                // the rest of this word is ~~garbage~~ reserved
                let cap_ptr = self.read_offset(0x34) as u8;
                // 0x38 is reserved
                let [max_latency, min_grant, irq_pin, irq_line] =
                    self.read_offset(0x3c).to_le_bytes();
                device::Kind::Standard(device::StandardDetails {
                    base_addrs,
                    cardbus_cis_ptr,
                    subsystem,
                    exp_rom_base_addr,
                    cap_ptr,
                    _res0: [0; 7],
                    irq_line,
                    irq_pin,
                    min_grant,
                    max_latency,
                })
            }
            Ok(device::HeaderType::CardBusBridge) => {
                tracing::debug!(device = ?self.addr, "skipping CardBus-to-PCI bridge; not yet implemented");
                return None;
            }
            Ok(device::HeaderType::PciBridge) => {
                tracing::debug!(device = ?self.addr, "skipping PCI-to-PCI bridge; not yet implemented");
                return None;
            }
            Err(err) => {
                tracing::warn!(device = ?self.addr, %err, "invalid header type! skipping device");
                return None;
            }
        };

        Some(device::Device { header, details })
    }

    pub fn read_header(&self) -> Option<device::Header> {
        // Off | Bits 31-24    | Bits 23-16    | Bits 15-8     | Bits 7-0      |
        // 0x0 | Device ID                     | Vendor ID                     |
        // 0x4 | Status                        | Command                       |
        // 0x8 | Class code   | Subclass       | Prog IF        | Revision ID  |
        // 0xC | BIST         | Header type    | Latency Timer  | Cacheline Sz |
        let id = self.read_device_id()?;
        let (status, command) = {
            let word = self.read_offset(0x4);
            let status = (word >> 16) as u16;
            let command = device::CommandReg((word & 0xFFFF) as u16);
            (status, command)
        };

        let [revision_id, prog_if, subclass, class] = self.read_offset(0x8).to_le_bytes();
        let class = device::RawClasses { class, subclass };

        let [cache_line_size, latency_timer, header_type, bist] =
            self.read_offset(0xC).to_le_bytes();
        let header_type = device::HeaderTypeReg::from_bits(header_type);
        let bist = device::BistReg(bist);

        Some(device::Header {
            id,
            command,
            status,
            revision_id,
            prog_if,
            class,
            cache_line_size,
            latency_timer,
            header_type,
            bist,
        })
    }

    pub fn read_device_id(&self) -> Option<device::Id> {
        let id = self.read_offset(0);
        let vendor_id = (id & 0xFFFF) as u16;

        // vendor ID 0xFFFF is reserved for nonexistent devices.
        if vendor_id == 0xFFFF {
            return None;
        }

        Some(device::Id {
            vendor_id,
            device_id: (id >> 16) as u16,
        })
    }

    pub fn read_header_type(&self) -> device::HeaderTypeReg {
        let bits = self.read_offset(0x0C);
        let bits = bits & 0xffff << 16;
        device::HeaderTypeReg::from_bits(bits as u8)
    }
}

const ADDRESS_PORT: u16 = 0xCF8;
const DATA_PORT: u16 = 0xCFC;

impl ConfigAddressBits {
    fn from_address(
        ConfigAddress {
            bus,
            device,
            function,
        }: ConfigAddress,
    ) -> Self {
        Self::new()
            .with(Self::BUS, bus)
            .with(Self::DEVICE, device as u32)
            .with(Self::FUNCTION, function as u32)
            .with(Self::ENABLE, true)
    }
}

impl fmt::Display for ConfigAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            bus,
            device,
            function,
        } = self;
        write!(f, "{bus:04x}:{device:02x}:{function:02x}",)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_addr_is_valid() {
        ConfigAddressBits::assert_valid();
        assert_eq!(ConfigAddressBits::ENABLE.least_significant_index(), 31);
    }
}
