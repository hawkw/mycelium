// TODO(eliza): write a `RwLock`...
use crate::drivers::pci::*;

pub fn init_pci() {
    let mut bad = 0;
    let mut devices = DeviceRegistry::default();

    let _span = tracing::info_span!("enumerating PCI devices").entered();
    for (addr, config) in config::enumerate_all() {
        let class = match config.header.classes() {
            Ok(class) => class,
            Err(error) => {
                tracing::error!(
                    target: "pci",
                    error = %error,
                    "[{addr}] bad class"
                );
                bad += 1;
                continue;
            }
        };

        let _ = tracing::info_span!(
            target: "pci",
            "pci",
            class = %class.class().name(),
            subclass = %class.subclass().name(),
            "[{addr}]"
        )
        .entered();

        let id = config.header.id;
        match config.header.id() {
            Ok(ids) => {
                tracing::info!(
                    target: "  pci",
                    vendor = %ids.vendor().name(),
                    device = %ids.name(),
                );
            }
            Err(_) => {
                tracing::warn!(
                    target: "  pci",
                    vendor = id.vendor_id,
                    device = id.device_id,
                    "unrecognized vendor or device ID"
                );
            }
        };

        assert!(
            devices.insert(addr, class, id),
            "PCI device inserted twice! device={:?}",
            config
        );
    }

    tracing::info!("found {} PCI devices ({bad} bad)", devices.len());

    DEVICES.init(devices);
}
