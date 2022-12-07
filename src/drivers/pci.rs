use alloc::collections::{
    btree_set::{self, BTreeSet},
    BTreeMap,
};
pub use mycelium_pci::*;
use mycelium_util::sync::InitOnce;

#[derive(Debug, Default)]
pub struct DeviceRegistry {
    // TODO(eliza): maybe consider un-nesting the `Class` enum so you can just
    // say "give me all mass storage devices"...
    by_class: BTreeMap<Class, Devices>,
    by_vendor: BTreeMap<u16, BTreeMap<u16, Devices>>,
    len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Devices(BTreeSet<Address>);

pub static DEVICES: InitOnce<DeviceRegistry> = InitOnce::uninitialized();

impl DeviceRegistry {
    pub fn insert(
        &mut self,
        addr: Address,
        class: Class,
        device::Id {
            vendor_id,
            device_id,
        }: device::Id,
    ) -> bool {
        let mut clobbered = self
            .by_class
            .entry(class)
            .or_insert_with(Default::default)
            .0
            .insert(addr);
        clobbered |= self
            .by_vendor
            .entry(vendor_id)
            .or_insert_with(Default::default)
            .entry(device_id)
            .or_insert_with(Default::default)
            .0
            .insert(addr);
        self.len += 1;
        clobbered
    }

    pub fn by_class(&self, class: Class) -> Option<&Devices> {
        self.by_class.get(&class)
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
