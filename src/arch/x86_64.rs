use bootloader_api::config::{BootloaderConfig, Mapping};
use hal_core::boot::BootInfo;
use hal_x86_64::{
    cpu::{self, local::GsLocalData},
    vga,
};
pub use hal_x86_64::{
    cpu::{entropy::seed_rng, local::LocalKey, wait_for_interrupt},
    mm, NAME,
};

mod acpi;
mod boot;
mod framebuf;
pub mod interrupt;
mod oops;
pub mod pci;
pub mod shell;
pub use self::{
    boot::ArchInfo,
    oops::{oops, Oops},
};

#[cfg(test)]
mod tests;

pub type MinPageSize = mm::size::Size4Kb;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    // the kernel is mapped into the higher half of the virtual address space.
    config.mappings.dynamic_range_start = Some(0xFFFF_8000_0000_0000);
    config.mappings.page_table_recursive = Some(Mapping::Dynamic);

    config
};

#[cfg(target_os = "none")]
bootloader_api::entry_point!(arch_entry, config = &BOOTLOADER_CONFIG);

pub fn arch_entry(info: &'static mut bootloader_api::BootInfo) -> ! {
    unsafe {
        cpu::intrinsics::cli();
    }

    if let Some(offset) = info.physical_memory_offset.into_option() {
        // Safety: i hate everything
        unsafe {
            vga::init_with_offset(offset);
        }
    }
    /* else {
        // lol we're hosed
    } */

    let (boot_info, archinfo) = boot::BootloaderApiBootInfo::from_bootloader(info);
    crate::kernel_start(boot_info, archinfo);
}

pub fn init(_info: &impl BootInfo, archinfo: &ArchInfo) {
    pci::init_pci();

    if let Some(rsdp) = archinfo.rsdp_addr {
        let acpi = acpi::acpi_tables(rsdp);
        let platform_info = acpi.and_then(|acpi| acpi.platform_info());
        match platform_info {
            Ok(platform) => {
                tracing::debug!("found ACPI platform info");
                interrupt::enable_hardware_interrupts(Some(&platform.interrupt_model));
                let topo = cpu::topology::Topology::from_acpi(&platform).unwrap();
                return;
            }
            Err(error) => tracing::warn!(?error, "missing ACPI platform info"),
        }
    } else {
        // TODO(eliza): try using MP Table to bringup application processors?
        tracing::warn!("no RSDP from bootloader, skipping SMP bringup");
    }

    // no ACPI
    interrupt::enable_hardware_interrupts(None);
}

// TODO(eliza): this is now in arch because it uses the serial port, would be
// nice if that was cross platform...
#[cfg(test)]
pub fn run_tests() {
    use hal_x86_64::serial;
    let com1 = serial::com1().expect("if we're running tests, there ought to be a serial port");
    let mk = || com1.lock();
    match mycotest::runner::run_tests(mk) {
        Ok(()) => qemu_exit(QemuExitCode::Success),
        Err(_) => qemu_exit(QemuExitCode::Failed),
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

/// Exit using `isa-debug-exit`, for use in tests.
///
/// NOTE: This is a temporary mechanism until we get proper shutdown implemented.
#[cfg(test)]
pub(crate) fn qemu_exit(exit_code: QemuExitCode) -> ! {
    let code = exit_code as u32;
    unsafe {
        cpu::Port::at(0xf4).writel(code);

        // If the previous line didn't immediately trigger shutdown, hang.
        cpu::halt()
    }
}
