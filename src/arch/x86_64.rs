use bootloader::bootinfo;
use core::sync::atomic::{AtomicUsize, Ordering};
use hal_core::{boot::BootInfo, mem, Address, PAddr, VAddr};
use hal_x86_64::vga;
pub use hal_x86_64::{interrupt, mm, NAME};

#[derive(Debug)]
pub struct RustbootBootInfo {
    inner: &'static bootinfo::BootInfo,
}

type MemRegionIter = core::slice::Iter<'static, bootinfo::MemoryRegion>;

impl BootInfo for RustbootBootInfo {
    // TODO(eliza): implement
    type MemoryMap = core::iter::Map<MemRegionIter, fn(&bootinfo::MemoryRegion) -> mem::Region>;

    type Writer = vga::Writer;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap {
        fn convert_region_kind(kind: bootinfo::MemoryRegionType) -> mem::RegionKind {
            match kind {
                bootinfo::MemoryRegionType::Usable => mem::RegionKind::FREE,
                bootinfo::MemoryRegionType::InUse => mem::RegionKind::USED,
                bootinfo::MemoryRegionType::Reserved => mem::RegionKind::USED,
                bootinfo::MemoryRegionType::AcpiReclaimable => mem::RegionKind::BOOT_RECLAIMABLE,
                bootinfo::MemoryRegionType::BadMemory => mem::RegionKind::BAD,
                bootinfo::MemoryRegionType::Kernel => mem::RegionKind::KERNEL,
                bootinfo::MemoryRegionType::KernelStack => mem::RegionKind::KERNEL,
                bootinfo::MemoryRegionType::PageTable => mem::RegionKind::PAGE_TABLE,
                bootinfo::MemoryRegionType::Bootloader => mem::RegionKind::BOOT,
                bootinfo::MemoryRegionType::BootInfo => mem::RegionKind::BOOT,
                _ => mem::RegionKind::UNKNOWN,
            }
        }

        fn convert_region(region: &bootinfo::MemoryRegion) -> mem::Region {
            let start = PAddr::from_u64(region.range.start_addr());
            let size = {
                let end = PAddr::from_u64(region.range.end_addr()).offset(1);
                assert!(start < end, "bad memory range from bootinfo!");
                let size = start.difference(end);
                assert!(size >= 0);
                size as usize + 1
            };
            let kind = convert_region_kind(region.region_type);
            mem::Region::new(start, size, kind)
        }
        (&self.inner.memory_map[..]).iter().map(convert_region)
    }

    fn writer(&self) -> Self::Writer {
        vga::writer()
    }

    fn subscriber(&self) -> Option<tracing::Dispatch> {
        Some(tracing::Dispatch::new(
            hal_x86_64::tracing::Subscriber::default(),
        ))
    }

    fn bootloader_name(&self) -> &str {
        "rust-bootloader"
    }

    fn phys_mem_offset(&self) -> VAddr {
        let phys_offset = VAddr::from_u64(self.inner.physical_memory_offset);
        let recursive_table = VAddr::from_u64(self.inner.recursive_page_table_addr);
        tracing::info!(?phys_offset, ?recursive_table);
        phys_offset
    }
}

pub(crate) static TIMER: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct InterruptHandlers;

impl hal_core::interrupt::Handlers for InterruptHandlers {
    fn page_fault<C>(cx: C)
    where
        C: hal_core::interrupt::ctx::Context + hal_core::interrupt::ctx::PageFault,
    {
        tracing::error!(registers = ?cx.registers(), "page fault");
        oops(&format_args!("  PAGE FAULT\n\n{}", cx.registers()))
    }

    fn code_fault<C>(cx: C)
    where
        C: hal_core::interrupt::ctx::Context + hal_core::interrupt::ctx::CodeFault,
    {
        tracing::error!(registers = ?cx.registers(), "code fault");
        oops(&format_args!("  CODE FAULT\n\n{}", cx.registers()))
    }

    fn double_fault<C>(cx: C)
    where
        C: hal_core::interrupt::ctx::Context + hal_core::interrupt::ctx::CodeFault,
    {
        tracing::error!(registers = ?cx.registers(), "double fault",);
        oops(&format_args!("  DOUBLE FAULT\n\n{}", cx.registers()))
    }

    fn timer_tick() {
        TIMER.fetch_add(1, Ordering::Relaxed);
    }

    fn keyboard_controller() {
        // load-bearing read - if we don't read from the keyboard controller it won't
        // send another interrupt on later keystrokes.
        //
        // 0x60 is a magic PC/AT number.
        let scancode = unsafe { hal_x86_64::cpu::Port::at(0x60).readb() };
        tracing::info!(
            // for now
            "got scancode {}. the time is now: {}",
            scancode,
            TIMER.load(Ordering::Relaxed)
        );
    }

    fn test_interrupt<C>(cx: C)
    where
        C: hal_core::interrupt::ctx::Context,
    {
        tracing::info!(registers=?cx.registers(), "lol im in ur test interrupt");
    }
}

#[no_mangle]
#[cfg(target_os = "none")]
pub extern "C" fn _start(info: &'static bootinfo::BootInfo) -> ! {
    let bootinfo = RustbootBootInfo { inner: info };
    crate::kernel_main(&bootinfo);
}

#[cold]
pub fn oops(cause: &dyn core::fmt::Display) -> ! {
    use core::fmt::Write;
    tracing::error!(%cause, "oopsing");
    unsafe { asm!("cli" :::: "volatile") }
    let mut vga = vga::writer();
    unsafe {
        // forcibly unlock the mutex around the VGA buffer, to avoid deadlocking
        // if it was already held when we oopsed.
        //
        // TODO(eliza): when we are capable of multiprocessing, we shouldn't do
        // this until other cores that might be holding the lock are already
        // killed.
        vga.force_unlock();
    }
    const RED_BG: vga::ColorSpec = vga::ColorSpec::new(vga::Color::White, vga::Color::Red);
    vga.set_color(RED_BG);
    vga.clear();
    let _ = vga.write_str("\n  ");
    vga.set_color(vga::ColorSpec::new(vga::Color::Red, vga::Color::White));
    let _ = vga.write_str("OOPSIE WOOPSIE");
    vga.set_color(RED_BG);

    let _ = writeln!(vga, "\n  uwu we did a widdle fucky-wucky!\n{}", cause);
    let _ = vga.write_str("\n  it will never be safe to turn off your computer.");

    #[cfg(test)]
    qemu_exit(QemuExitCode::Failed);

    #[cfg(not(test))]
    loop {
        unsafe {
            asm!("hlt" :::: "volatile");
        }
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
        asm!("out 0xf4, eax" :: "{eax}"(code) :: "intel","volatile");

        // If the previous line didn't immediately trigger shutdown, hang.
        asm!("cli" :::: "volatile");
        loop {
            asm!("hlt" :::: "volatile");
        }
    }
}
