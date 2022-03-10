use bootloader::boot_info;
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use core::{ptr, sync::atomic::AtomicPtr};
use hal_core::{boot::BootInfo, mem, Address, PAddr, VAddr};
use hal_x86_64::{cpu, interrupt::Registers as X64Registers, serial, vga};
pub use hal_x86_64::{interrupt, mm, NAME};
use mycelium_util::{fmt, sync::InitOnce};

mod framebuf;
use self::framebuf::FramebufWriter;

mod oops;
pub use self::oops::{oops, Oops};

pub type MinPageSize = mm::size::Size4Kb;

#[derive(Debug)]
pub struct RustbootBootInfo {
    inner: &'static boot_info::BootInfo,
    has_framebuffer: bool,
}
type MemRegionIter = core::slice::Iter<'static, boot_info::MemoryRegion>;

impl BootInfo for RustbootBootInfo {
    // TODO(eliza): implement
    type MemoryMap = core::iter::Map<MemRegionIter, fn(&boot_info::MemoryRegion) -> mem::Region>;

    type Writer = vga::Writer;

    type Framebuffer = FramebufWriter;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap {
        fn convert_region_kind(kind: boot_info::MemoryRegionKind) -> mem::RegionKind {
            match kind {
                boot_info::MemoryRegionKind::Usable => mem::RegionKind::FREE,
                // TODO(eliza): make known
                boot_info::MemoryRegionKind::UnknownUefi(_) => mem::RegionKind::UNKNOWN,
                boot_info::MemoryRegionKind::UnknownBios(_) => mem::RegionKind::UNKNOWN,
                boot_info::MemoryRegionKind::Bootloader => mem::RegionKind::BOOT,
                _ => mem::RegionKind::UNKNOWN,
            }
        }

        fn convert_region(region: &boot_info::MemoryRegion) -> mem::Region {
            let start = PAddr::from_u64(region.start);
            let size = {
                let end = PAddr::from_u64(region.end).offset(1);
                assert!(start < end, "bad memory range from boot_info!");
                let size = start.difference(end);
                assert!(size >= 0);
                size as usize + 1
            };
            let kind = convert_region_kind(region.kind);
            mem::Region::new(start, size, kind)
        }
        self.inner.memory_regions[..].iter().map(convert_region)
    }

    fn writer(&self) -> Self::Writer {
        vga::writer()
    }

    fn framebuffer(&self) -> Option<Self::Framebuffer> {
        if !self.has_framebuffer {
            return None;
        }

        Some(unsafe { framebuf::mk_framebuf() })
    }

    fn subscriber(&self) -> Option<tracing::Dispatch> {
        use mycelium_trace::{
            embedded_graphics::MakeTextWriter,
            writer::{self, MakeWriterExt},
            Subscriber,
        };

        static COLLECTOR: InitOnce<
            Subscriber<
                writer::WithMaxLevel<MakeTextWriter<FramebufWriter>>,
                Option<&'static serial::Port>,
            >,
        > = InitOnce::uninitialized();

        if !self.has_framebuffer {
            // TODO(eliza): we should probably write to just the serial port if
            // there's no framebuffer...
            return None;
        }

        let collector = COLLECTOR.get_or_else(|| {
            let display_writer = MakeTextWriter::new(|| unsafe { framebuf::mk_framebuf() })
                .with_max_level(tracing::Level::INFO);
            Subscriber::display_only(display_writer).with_serial(serial::com1())
        });

        Some(tracing::Dispatch::from_static(collector))
    }

    fn bootloader_name(&self) -> &str {
        "rust-bootloader"
    }

    fn init_paging(&self) {
        mm::init_paging(self.vm_offset())
    }
}

impl RustbootBootInfo {
    fn vm_offset(&self) -> VAddr {
        VAddr::from_u64(
            self.inner
                .physical_memory_offset
                .into_option()
                .expect("haha wtf"),
        )
    }

    fn from_bootloader(inner: &'static mut boot_info::BootInfo) -> Self {
        let has_framebuffer = framebuf::init(inner);

        Self {
            inner,
            has_framebuffer,
        }
    }
}

static TEST_INTERRUPT_WAS_FIRED: AtomicUsize = AtomicUsize::new(0);

pub(crate) static TIMER: AtomicUsize = AtomicUsize::new(0);
pub(crate) struct InterruptHandlers;

/// Forcibly unlock the IOs we write to in an oops (VGA buffer and COM1 serial
/// port) to prevent deadlocks if the oops occured while either was locked.
///
/// # Safety
///
///  /!\ only call this when oopsing!!! /!\
impl hal_core::interrupt::Handlers<X64Registers> for InterruptHandlers {
    fn page_fault<C>(cx: C)
    where
        C: hal_core::interrupt::Context<Registers = X64Registers>
            + hal_core::interrupt::ctx::PageFault,
    {
        oops(Oops::fault(&cx, "PAGE FAULT"))
    }

    fn code_fault<C>(cx: C)
    where
        C: hal_core::interrupt::Context<Registers = X64Registers>
            + hal_core::interrupt::ctx::CodeFault,
    {
        oops(Oops::fault(&cx, "CODE FAULT"));
    }

    fn double_fault<C>(cx: C)
    where
        C: hal_core::interrupt::Context<Registers = X64Registers>
            + hal_core::interrupt::ctx::CodeFault,
    {
        oops(Oops::fault(&cx, "DOUBLE FAULT"))
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
        C: hal_core::interrupt::ctx::Context<Registers = X64Registers>,
    {
        let fired = TEST_INTERRUPT_WAS_FIRED.fetch_add(1, Ordering::Release) + 1;
        tracing::info!(registers = ?cx.registers(), fired, "lol im in ur test interrupt");
    }
}

#[cfg(target_os = "none")]
bootloader::entry_point!(arch_entry);

pub fn arch_entry(info: &'static mut boot_info::BootInfo) -> ! {
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

    let boot_info = RustbootBootInfo::from_bootloader(info);
    crate::kernel_main(&boot_info);
}

// TODO(eliza): this is now in arch because it uses the serial port, would be
// nice if that was cross platform...
#[cfg(test)]
pub fn run_tests() {
    let com1 = serial::com1().expect("if we're running tests, there ought to be a serial port");
    let mk = || com1.lock();
    match mycotest::runner::run_tests(mk) {
        Ok(()) => qemu_exit(QemuExitCode::Success),
        Err(()) => qemu_exit(QemuExitCode::Failed),
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

mycotest::decl_test! {
    fn interrupts_work() -> Result<(), &'static str> {
        let test_interrupt_fires = TEST_INTERRUPT_WAS_FIRED.load(Ordering::Acquire);

        tracing::debug!("testing interrupts...");
        interrupt::fire_test_interrupt();
        tracing::debug!("it worked");

        if TEST_INTERRUPT_WAS_FIRED.load(Ordering::Acquire) != test_interrupt_fires + 1 {
            Err("test interrupt wasn't fired")
        } else {
            Ok(())
        }
    }
}

mycotest::decl_test! {
    fn alloc_some_4k_pages() -> Result<(), hal_core::mem::page::AllocErr> {
        use hal_core::mem::page::Alloc;
        let page1 = tracing::info_span!("alloc page 1").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        let page2 = tracing::info_span!("alloc page 2").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page1, page2);
        tracing::info_span!("dealloc page 1").in_scope(|| {
            let res = crate::ALLOC.dealloc(page1);
            tracing::info!(?res, "deallocated page 1");
            res
        })?;
        let page3 = tracing::info_span!("alloc page 3").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page2, page3);
        tracing::info_span!("dealloc page 2").in_scope(|| {
            let res = crate::ALLOC.dealloc(page2);
            tracing::info!(?res, "deallocated page 2");
            res
        })?;
        let page4 = tracing::info_span!("alloc page 4").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page3, page4);
        tracing::info_span!("dealloc page 3").in_scope(|| {
            let res = crate::ALLOC.dealloc(page3);
            tracing::info!(?res, "deallocated page 3");
            res
        })?;
        tracing::info_span!("dealloc page 4").in_scope(|| {
            let res = crate::ALLOC.dealloc(page4);
            tracing::info!(?res, "deallocated page 4");
            res
        })
    }
}

mycotest::decl_test! {
    fn alloc_4k_pages_and_ranges() -> Result<(), hal_core::mem::page::AllocErr> {
        use hal_core::mem::page::Alloc;
        let range1 = tracing::info_span!("alloc range 1").in_scope(|| {
            let res = crate::ALLOC.alloc_range(mm::size::Size4Kb, 16);
            tracing::info!(?res);
            res
        })?;
        let page2 = tracing::info_span!("alloc page 2").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        tracing::info_span!("dealloc range 1").in_scope(|| {
            let res = crate::ALLOC.dealloc_range(range1);
            tracing::info!(?res, "deallocated range 1");
            res
        })?;
        let range3 = tracing::info_span!("alloc range 3").in_scope(|| {
            let res = crate::ALLOC.alloc_range(mm::size::Size4Kb, 10);
            tracing::info!(?res);
            res
        })?;
        tracing::info_span!("dealloc page 2").in_scope(|| {
            let res = crate::ALLOC.dealloc(page2);
            tracing::info!(?res, "deallocated page 2");
            res
        })?;
        let range4 = tracing::info_span!("alloc range 4").in_scope(|| {
            let res = crate::ALLOC.alloc_range(mm::size::Size4Kb, 8);
            tracing::info!(?res);
            res
        })?;
        tracing::info_span!("dealloc range 3").in_scope(|| {
            let res = crate::ALLOC.dealloc_range(range3);
            tracing::info!(?res, "deallocated range 3");
            res
        })?;
        tracing::info_span!("dealloc range 4").in_scope(|| {
            let res = crate::ALLOC.dealloc_range(range4);
            tracing::info!(?res, "deallocated range 4");
            res
        })
    }
}

mycotest::decl_test! {
    fn alloc_some_pages() -> Result<(), hal_core::mem::page::AllocErr> {
        use hal_core::mem::page::Alloc;
        let page1 = tracing::info_span!("alloc page 1").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        let page2 = tracing::info_span!("alloc page 2").in_scope(|| {
            let res = crate::ALLOC.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page1, page2);
        tracing::info_span!("dealloc page 1").in_scope(|| {
            let res = crate::ALLOC.dealloc(page1);
            tracing::info!(?res, "deallocated page 1");
            res
        })?;
        // TODO(eliza): when 2mb pages work, test that too...
        // let page3 = tracing::info_span!("alloc page 3").in_scope(|| {
        //     let res = crate::ALLOC.alloc(mm::size::Size2Mb);
        //     tracing::info!(?res);
        //     res
        // })?;
        tracing::info_span!("dealloc page 2").in_scope(|| {
            let res = crate::ALLOC.dealloc(page2);
            tracing::info!(?res, "deallocated page 2");
            res
        })?;
        // let page4 = tracing::info_span!("alloc page 4").in_scope(|| {
        //     let res = crate::ALLOC.alloc(mm::size::Size2Mb);
        //     tracing::info!(?res);
        //     res
        // })?;
        // tracing::info_span!("dealloc page 3").in_scope(|| {
        //     let res = crate::ALLOC.dealloc(page3);
        //     tracing::info!(?res, "deallocated page 3");
        //     res
        // })?;
        // tracing::info_span!("dealloc page 4").in_scope(|| {
        //     let res = crate::ALLOC.dealloc(page4);
        //     tracing::info!(?res, "deallocated page 4");
        //     res
        // })?;
        Ok(())
    }
}
