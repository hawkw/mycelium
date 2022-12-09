use crate::shell;
use alloc::collections::{
    btree_map::{self, BTreeMap},
    btree_set::{self, BTreeSet},
};
pub use mycelium_pci::*;
use mycelium_util::{fmt, sync::InitOnce};

#[derive(Debug, Default)]
pub struct DeviceRegistry {
    // TODO(eliza): these BTreeMaps could be `[T; 256]`...
    by_class: BTreeMap<Class, BySubclass>,
    by_vendor: BTreeMap<u16, BTreeMap<u16, Devices>>,
    len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Devices(BTreeSet<Address>);

#[derive(Clone, Debug, Default)]
pub struct BySubclass(BTreeMap<Subclass, Devices>);

pub static DEVICES: InitOnce<DeviceRegistry> = InitOnce::uninitialized();

pub const LSPCI_CMD: shell::Command = shell::Command::new("lspci")
    .with_help("list PCI devices")
    .with_usage("[ADDRESS]")
    .with_subcommands(&[shell::Command::new("class")
        .with_help(
            "list PCI devices by class. if no class code is provided, lists all devices by class.",
        )
        .with_usage("[CLASS]")
        .with_fn(|ctx| {
            fn list_classes<'a>(classes: impl IntoIterator<Item = (Class, &'a BySubclass)>) {
                for (class, subclasses) in classes {
                    let _span = tracing::info_span!("class", message = %class.name()).entered();
                    for (subclass, devices) in subclasses {
                        let _span =
                            tracing::info_span!("subclass", message = %subclass.name()).entered();
                        for device in devices {
                            log_device(device)
                        }
                    }
                }
            }

            // if no class code was provided, list all classes
            if ctx.command().is_empty() {
                tracing::info!("listing all PCI devices by class");
                list_classes(DEVICES.get().classes());
                return Ok(());
            }

            // otherwise, list devices in the provided class.
            let class = {
                let class_code = ctx
                    .command()
                    .parse::<u8>()
                    .map_err(|_| ctx.invalid_argument("a PCI class must be a valid `u8` value"))?;
                Class::from_id(class_code)
                    .map_err(|_| ctx.invalid_argument("not a valid PCI class"))?
            };
            if let Some(subclasses) = DEVICES.get().class(&class) {
                list_classes(Some((class, subclasses)));
            } else {
                tracing::info!("no {} devices found", class.name());
            }

            Ok(())
        })])
    .with_fn(|ctx| {
        if !ctx.command().is_empty() {
            return Err(ctx.other_error("listing an individual PCI device is not yet implemented"));
        }

        return Err(ctx.other_error("listing PCI devices by address is not yet implemented."));
    });

fn log_device(device: Address) {
    let Some(header) = config::ConfigReg::new(device).read_header() else {
        tracing::error!(target: "pci", "[{device}]: invalid device header!");
        return;
    };
    match header.id() {
        Ok(id) => tracing::info!(
            target: "pci",
            vendor = %id.vendor().name(),
            device = %id.name(),
            prog_if = fmt::bin(header.prog_if),
            "[{device}]",
        ),
        Err(_) => tracing::warn!(
            target: "pci",
            vendor = fmt::hex(header.id.vendor_id),
            device = fmt::hex(header.id.device_id),
            prog_if = fmt::bin(header.prog_if),
            "[{device}]: unknown ID or vendor"
        ),
    }
}

impl DeviceRegistry {
    pub fn insert(
        &mut self,
        addr: Address,
        class: Classes,
        device::RawIds {
            device_id,
            vendor_id,
        }: device::RawIds,
    ) -> bool {
        let mut new = self
            .by_class
            .entry(class.class())
            .or_insert_with(Default::default)
            .0
            .entry(class.subclass())
            .or_insert_with(Default::default)
            .0
            .insert(addr);
        new &= self
            .by_vendor
            .entry(vendor_id)
            .or_insert_with(Default::default)
            .entry(device_id)
            .or_insert_with(Default::default)
            .0
            .insert(addr);
        self.len += 1;
        new
    }

    pub fn class(&self, class: &Class) -> Option<&BySubclass> {
        self.by_class.get(class)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        let is_empty = self.len == 0;
        debug_assert_eq!(is_empty, self.by_class.is_empty());
        debug_assert_eq!(is_empty, self.by_vendor.is_empty());
        is_empty
    }

    pub fn classes(&self) -> impl Iterator<Item = (Class, &BySubclass)> + '_ {
        self.by_class.iter().filter_map(|(class, by_subclass)| {
            if !by_subclass.0.is_empty() {
                Some((*class, by_subclass))
            } else {
                None
            }
        })
    }
}

// === impl BySubclass ===

impl BySubclass {
    pub fn subclass(&self, subclass: class::Subclass) -> Option<&Devices> {
        self.0.get(&subclass)
    }

    pub fn iter(
        &self,
    ) -> core::iter::Filter<
        btree_map::Iter<'_, Subclass, Devices>,
        fn(&(&Subclass, &Devices)) -> bool,
    > {
        self.0
            .iter()
            .filter(|&(subclass, devices)| !devices.is_empty())
    }
}

impl<'a> IntoIterator for &'a BySubclass {
    type Item = (&'a Subclass, &'a Devices);
    type IntoIter = core::iter::Filter<
        btree_map::Iter<'a, Subclass, Devices>,
        fn(&(&Subclass, &Devices)) -> bool,
    >;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// === impl Devices ===

impl Devices {
    pub fn iter(&self) -> core::iter::Copied<btree_set::Iter<'_, Address>> {
        self.0.iter().copied()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl<'a> IntoIterator for &'a Devices {
    type Item = Address;
    type IntoIter = core::iter::Copied<btree_set::Iter<'a, Address>>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
