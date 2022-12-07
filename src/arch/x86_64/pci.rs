// TODO(eliza): write a `RwLock`...
use crate::drivers::pci::*;

pub fn init_pci() {
    let mut bad = 0;
    let mut devices = DeviceRegistry::default();

    let _span = tracing::info_span!("enumerating PCI devices").entered();
    for (addr, config) in config::enumerate_all() {
        let id = config.header.id;
        let class = match config.header.class() {
            Ok(class) => class,
            Err(error) => {
                tracing::warn!(
                    target: "pci",
                    error = %error,
                    vendor = id.vendor_id,
                    device = id.device_id,
                    "[{addr}] bad class"
                );
                bad += 1;
                continue;
            }
        };

        tracing::info!(
            target: "pci",
            vendor = id.vendor_id,
            device = id.device_id,
            %class,
            "[{addr}]"
        );
        assert!(
            !devices.insert(addr, class, id),
            "PCI device inserted twice! device={:?}",
            config
        );
    }

    tracing::info!("found {} PCI devices ({bad} bad)", devices.len());

    DEVICES.init(devices);
}
