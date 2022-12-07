use alloc::collections::{
    btree_map::{self, BTreeMap},
    btree_set::{self, BTreeSet},
};
pub use mycelium_pci::*;
use mycelium_util::sync::InitOnce;

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
}

// === impl BySubclass ===

impl BySubclass {
    pub fn subclass(&self, subclass: class::Subclass) -> Option<&Devices> {
        self.0.get(&subclass)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Subclass, Address)> + '_ {
        self.0
            .iter()
            .flat_map(|(&subclass, devices)| devices.iter().map(move |device| (subclass, device)))
    }
}

// === impl Devices ===

impl Devices {
    pub fn iter(&self) -> core::iter::Copied<btree_set::Iter<'_, Address>> {
        self.0.iter().copied()
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
