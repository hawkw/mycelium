// TODO(eliza): write a `RwLock`...
use alloc::collections::{BTreeMap, BTreeSet};
pub use mycelium_pci::*;

#[derive(Debug)]
pub struct DeviceRegistry {
    by_class: BTreeMap<Class, Devices>,
    by_vendor: BTreeMap<u16, BTreeMap<u16, Devices>>,
    len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Devices(BTreeSet<Address>);

pub fn init_pci() -> DeviceRegistry {
    let mut bad = 0;
    let mut devices = DeviceRegistry {
        by_class: BTreeMap::new(),
        by_vendor: BTreeMap::new(),
        len: 0,
    };

    let _span = tracing::info_span!("enumerating PCI devices").entered();
    for (addr, config) in config::enumerate_all() {
        let class = match config.header.class() {
            Ok(class) => class,
            Err(error) => {
                tracing::warn!(
                    target: "pci",
                    error = %error,
                    vendor = config.header.id.vendor_id,
                    device = config.header.id.device_id,
                    "[{addr}] bad class"
                );
                bad += 1;
                continue;
            }
        };

        tracing::info!(
            target: "pci",
            vendor = config.header.id.vendor_id,
            device = config.header.id.device_id,
            %class,
            "[{addr}]"
        );
        devices.devices.len += 1;
    }

    tracing::info!("found {} PCI devices ({bad} bad)", devices.len);

    devices
}

impl DeviceRegistry {
    pub fn insert(&mut self, addr: Address, class: Class, id: device::Id) -> bool {
        self.by_class
            .entry(class)
            .or_insert(Devices::default())
            .insert(addr);
    }
}
