// TODO(eliza): write a `RwLock`...

pub fn init_pci() {
    // TODO(eliza): store the enumerated PCI devices somewhere lol
    let mut found = 0;
    let mut bad = 0;
    let _span = tracing::info_span!("enumerating PCI devices").entered();
    for (addr, config) in mycelium_pci::config::enumerate_all() {
        match config.header.class() {
            Ok(class) => tracing::info!(
                target: "pci",
                vendor = config.header.id.vendor_id,
                device = config.header.id.device_id,
                %class,
                "[{addr}]"
            ),
            Err(error) => {
                tracing::warn!(target: "pci",
                    error = %error,
                    vendor = config.header.id.vendor_id,
                    device = config.header.id.device_id,
                    "[{addr}] bad class"
                );
                bad += 1;
            }
        }
        found += 1;
    }
    tracing::info!("found {found} PCI devices ({bad} bad)",);
}
