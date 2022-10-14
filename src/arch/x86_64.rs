use hal_core::{boot::BootInfo, mem, PAddr, VAddr};
use hal_x86_64::{cpu, vga};
pub use hal_x86_64::{cpu::entropy::seed_rng, mm, NAME};
use mycelium_util::sync::InitOnce;

mod acpi;
mod bootloader;
mod framebuf;
pub mod interrupt;
mod oops;
pub mod pci;
pub use self::oops::{oops, Oops};

#[cfg(test)]
mod tests;

pub type MinPageSize = mm::size::Size4Kb;

pub use self::interrupt::init_interrupts;

pub fn tick_timer() {
    interrupt::TIMER.advance_ticks(0);
}

#[cfg(target_os = "none")]
bootloader::entry_point!(arch_entry);

pub fn arch_entry(info: &'static mut bootloader::boot_info::BootInfo) -> ! {
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

    let boot_info = bootloader::RustbootBootInfo::from_bootloader(info);
    crate::kernel_start(&boot_info);
}

pub fn arch_init(info: &impl BootInfo) {
    pci::init_pci();
}

// TODO(eliza): this is now in arch because it uses the serial port, would be
// nice if that was cross platform...
#[cfg(test)]
pub fn run_tests() {
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
