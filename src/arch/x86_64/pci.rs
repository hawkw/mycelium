pub fn init_pci() {
    // TODO(eliza): store the enumerated PCI devices somewhere lol
    let mut found = 0;
    let mut bad = 0;
    tracing::info!("enumerating PCI devices...");
    for (addr, config) in mycelium_pci::config::enumerate_all() {
        tracing::info!(
            target: " pci",
            vendor = config.header.id.vendor_id,
            device = config.header.id.device_id,
            "[{addr}]"
        );
        match config.header.class() {
            Ok(class) => tracing::info!(target: " pci", class = %class),
            Err(error) => {
                tracing::warn!(target: "pci", error = %error, "bad class");
                bad += 1;
            }
        }
        found += 1;
    }
    tracing::info!("found {found} PCI devices ({bad} bad)",);
}
