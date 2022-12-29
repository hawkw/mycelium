//! PCI Bus Configuration Space
//!
//! This module implements the PCI "configuration space access mechanism #1" as
//! described [on the OSDev Wiki here][wiki].
//!
//! [wiki]: https://wiki.osdev.org/Pci#Configuration_Space_Access_Mechanism_.231
use crate::{
    addr::AddressBits,
    class, device,
    error::{self, unexpected, UnexpectedValue},
    register, Address, Device,
};
use hal_x86_64::cpu::Port;
use mycelium_bitfield::{bitfield, pack};

#[derive(Debug)]
pub struct ConfigReg {
    data_port: Port,
    addr_port: Port,
    addr: ConfigAddress,
}

pub fn enumerate_bus(bus: u8) -> impl Iterator<Item = (Address, Device)> {
    let addrs = (0..32u8).map(move |device| Address::new().with_bus(bus).with_device(device));

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
                let addr = addr.with_function(function);
                Some((addr, ConfigReg::new(addr).read_device()?))
            });
            core::iter::once((addr, device)).chain(functions)
        })
}

/// Enumerate all PCI buses
pub fn enumerate_all() -> impl Iterator<Item = (Address, Device)> {
    (0..=255u8).flat_map(enumerate_bus)
}

bitfield! {
    /// The PCI bus `CONFIG_ADDRESS` register (port `0xCF8`).
    ///
    /// |Bit 31    |Bits 30-24|Bits 23-16|Bits 15-11   |Bits 10-8      |Bits 7-0       |
    /// |:---------|----------|----------|:------------|:--------------|:--------------|
    /// |Enable Bit|Reserved  |Bus Number|Device Number|Function Number|Register Offset|
    struct ConfigAddress<u32> {
        const REGISTER_OFFSET: u8;
        const FUNCTION = 3;
        const DEVICE = 5;
        const BUS: u8;
        const _RESERVED = 7;
        const ENABLE: bool;
    }
}

impl ConfigReg {
    pub fn new(addr: Address) -> Self {
        Self::try_from(addr).expect("invalid config register address")
    }

    pub fn try_from(addr: Address) -> Result<Self, UnexpectedValue<Address>> {
        Ok(Self {
            data_port: Port::at(DATA_PORT),
            addr_port: Port::at(ADDRESS_PORT),
            addr: ConfigAddress::from_address(addr)?,
        })
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

    pub fn read_command_status(&self) -> (register::Command, register::Status) {
        let word = self.read_command_status_reg();
        let command = word.get(register::RegisterWord::COMMAND);
        let status = word.get(register::RegisterWord::STATUS);
        (command, status)
    }

    fn read_command_status_reg(&self) -> register::RegisterWord {
        let word = self.read_offset(offsets::COMMAND_STATUS);
        register::RegisterWord::from_bits(word)
    }

    pub fn read_header(&self) -> Option<device::Header> {
        // Off | Bits 31-24    | Bits 23-16    | Bits 15-8     | Bits 7-0      |
        // 0x0 | Device ID                     | Vendor ID                     |
        // 0x4 | Status                        | Command                       |
        // 0x8 | Class code   | Subclass       | Prog IF        | Revision ID  |
        // 0xC | BIST         | Header type    | Latency Timer  | Cacheline Sz |
        let id = self.read_device_id()?;
        let (command, status) = self.read_command_status();
        let [revision_id, prog_if, subclass, class] = self.read_offset(0x8).to_le_bytes();
        let class = class::RawClasses { class, subclass };

        let [cache_line_size, latency_timer, header_type, bist] =
            self.read_offset(0xC).to_le_bytes();
        let header_type = device::HeaderTypeReg::from_bits(header_type);
        let bist = register::Bist::from_bits(bist);

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

    pub fn read_device_id(&self) -> Option<device::RawIds> {
        let id = self.read_offset(0);
        let vendor_id = (id & 0xFFFF) as u16;

        // vendor ID 0xFFFF is reserved for nonexistent devices.
        if vendor_id == 0xFFFF {
            return None;
        }

        Some(device::RawIds {
            vendor_id,
            device_id: (id >> 16) as u16,
        })
    }

    pub(crate) fn read_header_type(&self) -> device::HeaderTypeReg {
        let bits = self.read_offset(0x0C);
        let bits = bits & 0xffff << 16;
        device::HeaderTypeReg::from_bits(bits as u8)
    }

    pub fn send_command(
        &self,
        f: impl FnOnce(register::Status, register::Command) -> register::Command,
    ) {
        use register::RegisterWord;
        let word = self.read_command_status_reg();
        let command = f(
            word.get(RegisterWord::STATUS),
            word.get(RegisterWord::COMMAND),
        );
        self.write_offset(
            offsets::COMMAND_STATUS,
            word.with(RegisterWord::COMMAND, command).bits(),
        );
    }

    fn read_offset(&self, offset: u8) -> u32 {
        let addr = self.addr.with(ConfigAddress::REGISTER_OFFSET, offset);
        unsafe {
            self.addr_port.writel(addr.bits());
            self.data_port.readl()
        }
    }

    fn write_offset(&self, offset: u8, word: u32) {
        let addr = self.addr.with(ConfigAddress::REGISTER_OFFSET, offset);
        unsafe {
            self.addr_port.writel(addr.bits());
            self.data_port.writel(word)
        }
    }
}

const ADDRESS_PORT: u16 = 0xCF8;
const DATA_PORT: u16 = 0xCFC;

impl ConfigAddress {
    const BUS_PAIR: pack::Pair32 = Self::BUS
        .typed::<u32, ()>()
        .pair_with(AddressBits::BUS.typed::<_, AddressBits>());
    const DEVICE_PAIR: pack::Pair32 = Self::DEVICE.pair_with(AddressBits::DEVICE);
    const FUNCTION_PAIR: pack::Pair32 = Self::FUNCTION.pair_with(AddressBits::FUNCTION);

    fn from_address(addr: Address) -> Result<Self, error::UnexpectedValue<Address>> {
        if addr.group().is_some() {
            return Err(unexpected(addr)
                .named("only PCI Express addresses may contain extended segment groups"));
        }
        let addr_bits = addr.bitfield().bits();
        let bits = pack::Pack32::pack_in(0)
            .pack_from_src(addr_bits, &Self::BUS_PAIR)
            .pack_from_src(addr_bits, &Self::DEVICE_PAIR)
            .pack_from_src(addr_bits, &Self::FUNCTION_PAIR)
            .pack(true, &Self::ENABLE)
            .bits();
        Ok(Self::from_bits(bits))
    }
}

mod offsets {
    pub(super) const COMMAND_STATUS: u8 = 0x4;
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::{prop_assert, prop_assert_eq, proptest};

    #[test]
    fn config_addr_is_valid() {
        ConfigAddress::assert_valid();
        assert_eq!(ConfigAddress::ENABLE.least_significant_index(), 31);
    }

    #[test]
    fn config_addr_pairs_valid() {
        ConfigAddress::BUS_PAIR.assert_valid();
        ConfigAddress::DEVICE_PAIR.assert_valid();
        ConfigAddress::FUNCTION_PAIR.assert_valid();

        println!("BUS_PAIR: {:#?}", ConfigAddress::BUS_PAIR);
        println!("DEVICE_PAIR: {:#?}", ConfigAddress::DEVICE_PAIR);
        println!("FUNCTION_PAIR: {:#?}", ConfigAddress::FUNCTION_PAIR);
    }

    #[test]
    fn config_addr_fn_pair() {
        let addr = Address::new().with_function(3);
        let addr_bits = addr.bitfield().bits();
        let bits = pack::Pack32::pack_in(0)
            .pack_from_src(addr_bits, &ConfigAddress::FUNCTION_PAIR)
            .bits();
        let config_addr = ConfigAddress::from_bits(bits);
        assert_eq!(
            config_addr.get(ConfigAddress::FUNCTION),
            addr.bitfield().get(AddressBits::FUNCTION),
            "\n{config_addr}"
        );
    }

    proptest! {
        #[test]
        fn config_address_from_address(bus in 0u8..255u8, device in 0u8..32u8, function in 0u8..8u8) {
            let addr = Address::new().with_bus(bus).with_device(device).with_function(function);

            let config_addr = ConfigAddress::from_address(addr);
            prop_assert!(config_addr.is_ok(), "converting address {addr} to ConfigAddress failed unexpectedly");
            let config_addr = config_addr.unwrap();

            prop_assert_eq!(
                config_addr.get(ConfigAddress::BUS),
                bus,
                "\n  addr: {:?}\n   cfg:\n{}", addr, config_addr
            );
            prop_assert_eq!(
                config_addr.get(ConfigAddress::DEVICE) as u8,
                device,
                "\n  addr: {:?}\n   cfg:\n{}", addr, config_addr
            );
            prop_assert_eq!(
                config_addr.get(ConfigAddress::FUNCTION) as u8,
                function,
                "\n  addr: {:?}\n   cfg:\n{}", addr, config_addr
            );

        }
    }
}
