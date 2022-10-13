use acpi::AcpiHandler;
use core::ptr::NonNull;
use hal_core::PAddr;
use hal_x86_64::mm;

#[derive(Clone)]
struct IdentityMappedAcpiHandler;

impl AcpiHandler for IdentityMappedAcpiHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<Self, T> {
        let paddr = PAddr::from_u64(physical_address as u64);
        let vaddr = mm::kernel_vaddr_of(paddr);
        let vptr =
            NonNull::new(vaddr.as_ptr()).expect("virtual address for ACPI region is not null");
        // we have identity mapped all physical memory, so we don't actually
        // have to map any pages --- just tell the ACPI crate that it can use
        // this region.
        acpi::PhysicalMapping::new(physical_address, vptr, size, size, Self)
    }

    fn unmap_physical_region<T>(_region: &acpi::PhysicalMapping<Self, T>) {
        // we don't need to unmap anything, since we didn't map anything. :)
    }
}
