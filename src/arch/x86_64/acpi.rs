pub use acpi::AcpiError;
use acpi::{AcpiHandler, AcpiTables};
use core::{fmt, ptr::NonNull};
use hal_core::{Address, PAddr};
use hal_x86_64::mm;

#[derive(Debug)]
pub enum Error {
    Acpi(AcpiError),
    Other(&'static str),
}

pub(super) fn acpi_tables(
    rsdp_addr: PAddr,
) -> Result<AcpiTables<IdentityMappedAcpiHandler>, AcpiError> {
    tracing::info!("trying to parse ACPI tables from RSDP...");
    let tables = unsafe { AcpiTables::from_rsdp(IdentityMappedAcpiHandler, rsdp_addr.as_usize()) }?;
    tracing::info!("found ACPI tables!");
    Ok(tables)
}

#[tracing::instrument(err, skip(platform))]
pub fn bringup_smp(platform: &acpi::PlatformInfo) -> Result<(), Error> {
    use acpi::platform::{self, interrupt::InterruptModel};

    tracing::info!(?platform.power_profile);

    let apic = match platform.interrupt_model {
        acpi::InterruptModel::Apic(ref apic) => {
            tracing::info!("APIC interrupt model detected");
            apic
        }
        InterruptModel::Unknown => {
            return Err(Error::Other(
                "MADT does not indicate support for APIC interrupt model!",
            ));
        }
        ref model => {
            tracing::warn!(?model, "unknown interrupt model detected");
            return Err(Error::Other(
                "MADT does not indicate support for APIC interrupt model!",
            ));
        }
    };

    tracing::debug!(?apic);

    let platform::ProcessorInfo {
        ref application_processors,
        ref boot_processor,
    } = platform
        .processor_info
        .as_ref()
        .ok_or(Error::Other("no processor information found in MADT!"))?;
    tracing::info!("boot processor seems normalish");
    tracing::debug!(?boot_processor);
    tracing::info!(
        "found {} application processors",
        application_processors.len()
    );
    tracing::debug!(?application_processors);
    hal_x86_64::cpu::smp::bringup().unwrap();

    tracing::warn!("not starting app processors (SMP support isn't done yet)");

    Ok(())
}

#[derive(Clone)]
pub(super) struct IdentityMappedAcpiHandler;

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

// === impl Error ===

impl From<AcpiError> for Error {
    fn from(inner: AcpiError) -> Self {
        Self::Acpi(inner)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // format the ACPI error using its `fmt::Debug` implementation,
            // since the ACPI crate doesn't implement `fmt::Display` for its
            // errors.
            // TODO(eliza): add a `Display` impl upstream...
            Self::Acpi(inner) => write!(f, "ACPI error: {inner:?}"),
            Self::Other(msg) => fmt::Display::fmt(msg, f),
        }
    }
}
