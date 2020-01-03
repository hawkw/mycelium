use bootloader::bootinfo;
use hal_core::{boot::BootInfo, mem, Address};
use hal_x86_64::{vga, PAddr, X64};

#[derive(Debug)]
pub struct RustbootBootInfo {
    inner: &'static bootinfo::BootInfo,
}

type MemRegionIter = core::slice::Iter<'static, bootinfo::MemoryRegion>;

impl BootInfo for RustbootBootInfo {
    type Arch = X64;
    // TODO(eliza): implement
    type MemoryMap =
        core::iter::Map<MemRegionIter, fn(&bootinfo::MemoryRegion) -> mem::Region<X64>>;

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

        fn convert_region(region: &bootinfo::MemoryRegion) -> mem::Region<X64> {
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
}

#[no_mangle]
#[cfg(target_os = "none")]
pub extern "C" fn _start(info: &'static bootinfo::BootInfo) -> ! {
    let bootinfo = RustbootBootInfo { inner: info };
    mycelium_kernel::kernel_main(&bootinfo);
}

#[cold]
#[cfg(target_os = "none")]
pub(crate) fn oops(cause: &dyn core::fmt::Display) -> ! {
    use core::fmt::Write;

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
    let _ = writeln!(vga, "\n  uwu we did a widdle fucky-wucky!\n\n{:2>}", cause);
    let rflags: u64;
    let cr0: u64;
    let cr3: u64;
    unsafe {
        asm!("
            pushfq
            popq $0
            mov %cr0, $1
            mov %cr3, $2
            "
            : "=r"(rflags), "=r"(cr0), "=r"(cr3) :: "memory"
        );
    };
    let _ = writeln!(
        vga,
        "\n  cr0: {:#032b}\n  cr3: {:#032b}\n  rflags: {:#064b}",
        cr0, cr3, rflags
    );
    let _ = vga.write_str("\n\n  it will never be safe to turn off your computer.");

    loop {
        unsafe {
            asm!("hlt" :::: "volatile");
        }
    }
}
