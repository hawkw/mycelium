use core::ptr;

pub type StandardDevice = Device<StandardDetails>;
pub type PciBridgeDevice = Device<PciBridgeDetails>;
pub type CardBusDevice = Device<CardBusDetails>;

#[derive(Debug)]
#[repr(C, packed)]
pub struct Device<T> {
    header: Header,
    details: T,
}

#[derive(Debug)]
#[repr(C)]
pub struct Header {
    id: DeviceId,
    command: CommandReg,
    status: u16,
    revision_id: u8,
    prog_if: u8,
    class: Class,
    cache_line_size: u8,
    latency_timer: u8,
    header_type: HeaderType,
    bist: BistReg,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct CommandReg(u16);

#[derive(Debug)]
#[repr(C)]
pub struct StandardDetails {
    base_addrs: [u32; 6],
    cardbus_cis_ptr: u32,
    subsystem: SubsystemId,
    exp_rom_base_addr: u32,
    cap_ptr: u8,
    _res0: [u8; 7],
    irq_line: u8,
    irq_pin: u8,
    min_grant: u8,
    min_latency: u8,
}

#[derive(Debug)]
#[repr(C)]
pub struct PciBridgeDetails {
    base_addrs: [u32; 2],
    // WIP
}

#[derive(Debug)]
#[repr(C)]
pub struct CardBusDetails {
    // WIP
}


#[derive(Debug, Copy)]
#[repr(transparent)]
pub struct HeaderType(u8);

#[derive(Debug)]
#[repr(C)]
pub struct DeviceId {
    vendor_id: u16,
    device_id: u16,
}

#[derive(Debug)]
#[repr(C)]
pub struct SubsystemId {
    vendor_id: u16,
    subsystem: u16,
}

#[derive(Debug)]
#[repr(C)]
pub struct Class {
    subclass: u8,
    class: u8,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct BistReg(u8);

impl HeaderType {
    const MF_BIT: u8 = 0b1000_0000;
    const TYPE_MASK: u8 = 0b0111_1111;
    const TYPE_STANDARD: u8 = 0x00h;
    const TYPE_PCI_BRIDGE: u8 = 0x01h;
    const TYPE_CARDBUS_BRIDGE: u8 = 0x02h;

    pub fn is_standard(self) -> bool {
        (self.0 & Self::TYPE_MASK) == Self::TYPE_STANDARD
    }

    pub fn is_pci_bridge(self) -> bool {
        (self.0 & Self::TYPE_MASK) == Self::TYPE_PCI_BRIDGE
    }

    pub fn is_cardbus_bridge(self) -> bool {
        (self.0 & Self::TYPE_MASK) == Self::TYPE_CARDBUS_BRIDGE
    }

    pub fn is_multifunction(self) -> bool {
        self.0 & Self::MF_BIT == 1
    }
}


impl BistReg {
    const CAPABLE_BIT: u8 = 0b1000_000;
    const START_BIT: u8 = 0b0100_000;
    const COMPLETION_MASK: u8 = 0b0000_0111;

    pub fn is_bist_capable(&self) -> bool {
        (*self.0) & Self::CAPABLE_BIT == 1
    }

    pub fn start_bist(&mut self) {
        let val = (*self.0) | Self::START_BIT;
        let ptr = (&mut self.0) as *mut u8;
        unsafe {
            ptr::write_volatile(ptr, val);
        }
    }

    pub fn completion_code(&self) -> u8 {
        (self.0) & Self::COMPLETION_MASK
    }
}

impl CommandReg {
    pub fn disconnect(&mut self) {
        unsafe {
            self.send_command(0);
        }
    }

    pub unsafe fn send_command(&mut self, command: u16) {
        ptr::write_volatile((&mut self.0) as *mut u16, command)
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
