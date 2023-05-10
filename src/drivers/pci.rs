use crate::shell;
use alloc::collections::{
    btree_map::{self, BTreeMap},
    btree_set::{self, BTreeSet},
};
use core::{iter, num::NonZeroU16};
pub use mycelium_pci::*;
use mycelium_util::{fmt, sync::InitOnce};

#[derive(Debug, Default)]
pub struct DeviceRegistry {
    // TODO(eliza): these BTreeMaps could be `[T; 256]`...
    by_class: BTreeMap<Class, BySubclass>,
    by_vendor: BTreeMap<u16, BTreeMap<u16, Devices>>,
    by_bus_group: BTreeMap<u16, BusGroup>,
    len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Devices(BTreeSet<Address>);

#[derive(Clone, Debug, Default)]
pub struct BySubclass(BTreeMap<Subclass, Devices>);

#[derive(Clone, Debug, Default)]
pub struct BusGroup(BTreeMap<u8, Bus>);

#[derive(Clone, Debug, Default)]
pub struct Bus([Option<BusDevice>; 32]);

type BusDevice = [Option<(Subclass, device::Id)>; 8];

pub static DEVICES: InitOnce<DeviceRegistry> = InitOnce::uninitialized();

type BusDeviceFilter = fn((usize, &Option<BusDevice>)) -> Option<(usize, &BusDevice)>;
type SubclassDeviceFilter = fn(&(&Subclass, &Devices)) -> bool;

pub const LSPCI_CMD: shell::Command = shell::Command::new("lspci")
    .with_help("list PCI devices")
    .with_usage("[ADDRESS]")
    .with_subcommands(&[shell::Command::new("class")
        .with_help(
            "list PCI devices by class. if no class code is provided, lists all devices by class.",
        )
        .with_usage("[CLASS]")
        .with_fn(|ctx| {
            fn log_device(device: Address, subclass: Subclass) {
                let Some(header) = config::ConfigReg::new(device).read_header() else {
                    tracing::error!(target: "pci", "[{device}]: invalid device header!");
                    return;
                };
                let prog_if = subclass.prog_if(header.raw_prog_if());
                match header.id() {
                    device::Id::Known(id) => tracing::info!(
                        target: "pci",
                        vendor = %id.vendor().name(),
                        device = %id.name(),
                        %prog_if,
                        "[{device}]",
                    ),
                    device::Id::Unknown(id) => tracing::warn!(
                        target: "pci",
                        vendor = fmt::hex(id.vendor_id),
                        device = fmt::hex(id.device_id),
                        %prog_if,
                        "[{device}]: unknown ID or vendor"
                    ),
                }
            }

            fn list_classes<'a>(classes: impl IntoIterator<Item = (Class, &'a BySubclass)>) {
                for (class, subclasses) in classes {
                    let _span = tracing::info_span!("class", message = %class.name()).entered();
                    for (subclass, devices) in subclasses {
                        let _span =
                            tracing::info_span!("subclass", message = %subclass.name()).entered();
                        for device in devices {
                            log_device(device, *subclass)
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
            let _addr = match ctx.command().parse::<Address>() {
                Ok(addr) => addr,
                Err(error) => {
                    tracing::error!(%error, "invalid PCI address");
                    return Err(ctx.invalid_argument("invalid PCI address"));
                }
            };

            return Err(ctx.other_error("looking up individual devices is not yet implemented"));
        }

        tracing::info!("listing all PCI devices by address");
        for (bus_group, group) in DEVICES.get().bus_groups() {
            let _span = tracing::info_span!("bus group", "{bus_group:04x}").entered();
            for (bus_num, bus) in group.buses() {
                let _span = tracing::info_span!("bus", "{bus_num:02x}").entered();
                for (device_num, device) in bus.devices() {
                    let _span = tracing::info_span!("device", "{device_num:02x}").entered();
                    for (fn_num, (subclass, id)) in device
                        .iter()
                        .enumerate()
                        .filter_map(|(fn_num, func)| Some((fn_num, func.as_ref()?)))
                    {
                        match id {
                            device::Id::Known(id) => tracing::info!(
                                target: " pci",
                                class = %subclass.class().name(),
                                device = %subclass.name(),
                                vendor = %id.vendor().name(),
                                device = %id.name(),
                                "[{bus_group:04x}:{bus_num:02x}:{device_num:02x}.{fn_num}]"
                            ),
                            device::Id::Unknown(id) => tracing::warn!(
                                target: " pci",
                                class = %subclass.class().name(),
                                device = %subclass.name(),
                                vendor = fmt::hex(id.vendor_id),
                                device = fmt::hex(id.device_id),
                                "[{bus_group:04x}:{bus_num:02x}:{device_num:02x}.{fn_num}]",
                            ),
                        }
                    }
                }
            }
        }

        Ok(())
    });

impl DeviceRegistry {
    pub fn insert(&mut self, addr: Address, class: Classes, id: device::Id) -> bool {
        // class->subclass->addr registry
        let mut new = self
            .by_class
            .entry(class.class())
            .or_default()
            .0
            .entry(class.subclass())
            .or_default()
            .0
            .insert(addr);
        // vendor->device->addr registry
        new &= self
            .by_vendor
            .entry(id.vendor_id())
            .or_default()
            .entry(id.device_id())
            .or_default()
            .0
            .insert(addr);
        // address (bus group->bus->device->function) registry
        let Bus(bus) = self
            .by_bus_group
            .entry(addr.group().map(Into::into).unwrap_or(0))
            .or_default()
            .0
            .entry(addr.bus())
            .or_default();
        let bus_device = bus
            .get_mut(addr.device() as usize)
            .expect("invalid address: the device must be 5 bits")
            .get_or_insert_with(|| [None; 8]);
        new &= bus_device
            .get_mut(addr.function() as usize)
            .expect("invalid address: the function must be 3 bits")
            .replace((class.subclass(), id))
            .is_none();
        new
    }

    pub fn class(&self, class: &Class) -> Option<&BySubclass> {
        self.by_class.get(class)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        let is_empty = self.len() == 0;
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

    pub fn bus_groups(&self) -> btree_map::Iter<'_, u16, BusGroup> {
        self.by_bus_group.iter()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Address, Subclass, device::Id)> + '_ {
        self.bus_groups().flat_map(|(&group_addr, group)| {
            group.buses().flat_map(move |(&bus_addr, bus)| {
                bus.devices().flat_map(move |(device_addr, device)| {
                    device
                        .iter()
                        .enumerate()
                        .filter_map(move |(func_num, func)| {
                            let (subclass, id) = func.as_ref()?;
                            let addr = Address::new()
                                .with_group(NonZeroU16::new(group_addr))
                                .with_bus(bus_addr)
                                .with_device(device_addr as u8)
                                .with_function(func_num as u8);
                            Some((addr, *subclass, *id))
                        })
                })
            })
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
    ) -> iter::Filter<btree_map::Iter<'_, Subclass, Devices>, SubclassDeviceFilter> {
        self.0.iter().filter(|&(_, devices)| !devices.is_empty())
    }
}

impl<'a> IntoIterator for &'a BySubclass {
    type Item = (&'a Subclass, &'a Devices);
    type IntoIter = iter::Filter<btree_map::Iter<'a, Subclass, Devices>, SubclassDeviceFilter>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// === impl Devices ===

impl Devices {
    pub fn iter(&self) -> iter::Copied<btree_set::Iter<'_, Address>> {
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
    type IntoIter = iter::Copied<btree_set::Iter<'a, Address>>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// === impl BusGroup ===

impl BusGroup {
    pub fn buses(&self) -> btree_map::Iter<'_, u8, Bus> {
        self.0.iter()
    }
}

// === impl Bus ===

impl Bus {
    pub fn devices(
        &self,
    ) -> iter::FilterMap<iter::Enumerate<core::slice::Iter<'_, Option<BusDevice>>>, BusDeviceFilter>
    {
        self.0
            .iter()
            .enumerate()
            .filter_map((|(addr, device)| Some((addr, device.as_ref()?))) as BusDeviceFilter)
    }
}
