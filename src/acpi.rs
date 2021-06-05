use crate::arch::mm;
use acpi::{AcpiHandler, PhysicalMapping};
use core::ptr;
use hal_core::{
    mem::page::{Map, Page, Size},
    Address, PAddr, VAddr,
};

pub type Tables = acpi::AcpiTables<AcpiMapper>;

#[derive(Debug, Copy, Clone)]
pub struct AcpiMapper;

impl AcpiHandler for AcpiMapper {
    #[tracing::instrument]
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        tracing::trace!("mapping physical region for ACPI");
        let start_page =
            Page::<PAddr, mm::MinPageSize>::containing_fixed(PAddr::from_usize(physical_address));
        tracing::trace!(?start_page);
        let end_page = Page::<PAddr, mm::MinPageSize>::containing_fixed(PAddr::from_usize(
            physical_address + size,
        ));
        tracing::trace!(?end_page);

        let mut ctrl = mm::PageCtrl::current();
        if start_page == end_page {
            tracing::debug!("ACPI region is contained within one page...");

            let virt = ctrl
                .identity_map(start_page, &crate::PAGE_ALLOCATOR)
                .commit();
            return PhysicalMapping {
                physical_start: start_page.base_addr().as_usize(),
                virtual_start: ptr::NonNull::new(virt.base_addr().as_ptr())
                    .expect("base of identity mapped range is not null lol"),
                region_length: size,
                mapped_length: virt.size().as_usize(),
                handler: *self,
            };
        }

        let range = start_page.range_inclusive(end_page);
        let virt_range = ctrl.identity_map_range(range, |_| {}, &crate::PAGE_ALLOCATOR);
        PhysicalMapping {
            physical_start: range.base_addr().as_usize(),
            virtual_start: ptr::NonNull::new(virt_range.base_addr().as_ptr())
                .expect("base of identity mapped range is not null lol"),
            region_length: size,
            mapped_length: virt_range.size(),
            handler: *self,
        }
    }

    fn unmap_physical_region<T>(&self, region: &PhysicalMapping<Self, T>) {
        let base_vaddr = VAddr::from_usize(region.virtual_start.as_ptr() as *const _ as usize);
        let start_page = Page::<VAddr, mm::MinPageSize>::starting_at_fixed(base_vaddr)
            .expect("not page aligned");
        let end_vaddr = base_vaddr.offset(region.mapped_length as i32 /* lol */);
        let end_page = Page::<VAddr, mm::MinPageSize>::starting_at_fixed(end_vaddr)
            .expect("not page aligned lol");
        let range = start_page.range_to(end_page);
        unsafe { mm::PageCtrl::current().unmap_range(range) }
    }
}
