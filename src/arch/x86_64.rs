use bootloader::boot_info::{self, FrameBuffer};
use core::sync::atomic::{AtomicUsize, Ordering};
use hal_core::{boot::BootInfo, mem, Address, PAddr, VAddr};
use hal_x86_64::{
    cpu,
    framebuffer::{self, Framebuffer},
    interrupt::Registers as X64Registers,
    serial, vga,
};
pub use hal_x86_64::{interrupt, mm, NAME};
use mycelium_util::sync::{spin, InitOnce};

#[cfg(test)]
use core::{ptr, sync::atomic::AtomicPtr};

pub type MinPageSize = mm::size::Size4Kb;

#[derive(Debug)]
pub struct RustbootBootInfo {
    inner: &'static boot_info::BootInfo,
    has_framebuffer: bool,
}

#[derive(Debug)]
pub struct LockedFramebuf(boot_info::FrameBuffer);

type MemRegionIter = core::slice::Iter<'static, boot_info::MemoryRegion>;

impl BootInfo for RustbootBootInfo {
    // TODO(eliza): implement
    type MemoryMap = core::iter::Map<MemRegionIter, fn(&boot_info::MemoryRegion) -> mem::Region>;

    type Writer = vga::Writer;

    type Framebuffer = Framebuffer<'static, spin::MutexGuard<'static, LockedFramebuf>>;

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
        (&self.inner.memory_regions[..]).iter().map(convert_region)
    }

    fn writer(&self) -> Self::Writer {
        vga::writer()
    }

    fn framebuffer(&self) -> Option<Self::Framebuffer> {
        if !self.has_framebuffer {
            return None;
        }

        Some(unsafe { Self::mk_framebuf() })
    }

    fn subscriber(&self) -> Option<tracing::Dispatch> {
        use mycelium_trace::{
            writer::{self, MakeWriterExt},
            Subscriber,
        };
        if !self.has_framebuffer {
            return None;
        }

        let display_writer = mycelium_trace::embedded_graphics::MakeTextWriter::new(|| unsafe {
            Self::mk_framebuf()
        })
        .with_max_level(tracing::Level::INFO);
        let display = Subscriber::display_only(display_writer);
        let dispatch = if let Some(serial) = serial::com1() {
            tracing::Dispatch::new(display.with_serial(serial))
        } else {
            tracing::Dispatch::new(display)
        };
        Some(dispatch)
    }

    fn bootloader_name(&self) -> &str {
        "rust-bootloader"
    }

    fn init_paging(&self) {
        mm::init_paging(self.vm_offset())
    }
}

impl RustbootBootInfo {
    unsafe fn mk_framebuf() -> Framebuffer<'static, spin::MutexGuard<'static, LockedFramebuf>> {
        let (cfg, buf) = unsafe {
            // Safety: we can reasonably assume this will only be called
            // after `arch_entry`, so if we've failed to initialize the
            // framebuffer...things have gone horribly wrong...
            FRAMEBUFFER.get_unchecked()
        };
        buf.force_unlock();
        Framebuffer::new(cfg, buf.lock())
    }

    fn vm_offset(&self) -> VAddr {
        VAddr::from_u64(
            self.inner
                .physical_memory_offset
                .into_option()
                .expect("haha wtf"),
        )
    }

    fn from_bootloader(inner: &'static mut boot_info::BootInfo) -> Self {
        let framebuffer = core::mem::replace(&mut inner.framebuffer, boot_info::Optional::None);
        let has_framebuffer = if let boot_info::Optional::Some(framebuffer) = framebuffer {
            let info = framebuffer.info();
            let cfg = framebuffer::Config {
                height: info.vertical_resolution,
                width: info.horizontal_resolution,
                px_bytes: info.bytes_per_pixel,
                line_len: info.stride,
                px_kind: match info.pixel_format {
                    boot_info::PixelFormat::RGB => framebuffer::PixelKind::Rgb,
                    boot_info::PixelFormat::BGR => framebuffer::PixelKind::Bgr,
                    boot_info::PixelFormat::U8 => framebuffer::PixelKind::Gray,
                    x => unimplemented!("hahaha wtf, found a weird pixel format: {:?}", x),
                },
            };
            FRAMEBUFFER.init((cfg, spin::Mutex::new(LockedFramebuf(framebuffer))));
            true
        } else {
            false
        };

        Self {
            inner,
            has_framebuffer,
        }
    }
}

impl AsMut<[u8]> for LockedFramebuf {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.buffer_mut()
    }
}

static FRAMEBUFFER: InitOnce<(framebuffer::Config, spin::Mutex<LockedFramebuf>)> =
    InitOnce::uninitialized();
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
        oops(&"PAGE FAULT", Some(&cx));
    }

    fn code_fault<C>(cx: C)
    where
        C: hal_core::interrupt::Context<Registers = X64Registers>
            + hal_core::interrupt::ctx::CodeFault,
    {
        oops(&"CODE FAULT", Some(&cx));
    }

    fn double_fault<C>(cx: C)
    where
        C: hal_core::interrupt::Context<Registers = X64Registers>
            + hal_core::interrupt::ctx::CodeFault,
    {
        oops(&"DOUBLE FAULT", Some(&cx))
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

#[cold]
pub fn oops(
    cause: &dyn core::fmt::Display,
    fault: Option<&dyn hal_core::interrupt::ctx::Context<Registers = X64Registers>>,
) -> ! {
    use core::fmt::Write;
    let mut vga = vga::writer();

    // /!\ disable all interrupts, unlock everything to prevent deadlock /!\
    //
    // Safety: it is okay to do this because we are oopsing and everything
    // is going to die anyway.
    unsafe {
        // disable all interrupts.
        cpu::intrinsics::cli();

        // If the system has a COM1, unlock it.
        if let Some(com1) = serial::com1() {
            com1.force_unlock();
        }

        // unlock the VGA buffer.
        vga.force_unlock();

        // unlock the frame buffer
        if let Some((_, fb)) = FRAMEBUFFER.try_get() {
            fb.force_unlock();
        }
    }

    tracing::trace!(%cause, fault = fault.as_ref().map(|cx| tracing::field::debug(cx.registers())), "oops");
    let registers = if let Some(fault) = fault {
        let registers = fault.registers();
        tracing::error!(%cause, ?registers, "OOPS! a CPU fault occurred");
        Some(registers)
    } else {
        tracing::error!(%cause, "OOPS! a panic occurred", );
        None
    };

    const RED_BG: vga::ColorSpec = vga::ColorSpec::new(vga::Color::White, vga::Color::Red);
    vga.set_color(RED_BG);
    vga.clear();
    let _ = vga.write_str("\n  ");
    vga.set_color(vga::ColorSpec::new(vga::Color::Red, vga::Color::White));
    let _ = vga.write_str("OOPSIE WOOPSIE");
    vga.set_color(RED_BG);

    let _ = writeln!(vga, "\n  uwu we did a widdle fucky-wucky!\n{}", cause);
    if let Some(registers) = registers {
        let _ = writeln!(vga, "\n{}\n", registers);

        let fault_addr = registers.instruction_ptr.as_usize();
        use yaxpeax_arch::LengthedInstruction;
        let _ = writeln!(vga, "Disassembly:");
        let mut ptr = fault_addr as u64;
        let decoder = yaxpeax_x86::long_mode::InstDecoder::default();
        for _ in 0..10 {
            // Safety: who cares! At worst this might double-fault by reading past the end of valid
            // memory. whoopsie.
            let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, 16) };
            let _ = write!(vga, "  {:016x}: ", ptr).unwrap();
            match decoder.decode_slice(bytes) {
                Ok(inst) => {
                    let _ = writeln!(vga, "{}", inst);
                    ptr += inst.len();
                }
                Err(e) => {
                    let _ = writeln!(vga, "{}", e);
                    break;
                }
            }
        }
    }
    let _ = vga.write_str("\n  it will never be safe to turn off your computer.");

    #[cfg(test)]
    {
        if let Some(test) = ptr::NonNull::new(CURRENT_TEST.load(Ordering::Acquire)) {
            if let Some(com1) = serial::com1() {
                let test = unsafe { test.as_ref() };
                let failure = if fault.is_some() {
                    mycotest::Failure::Fault
                } else {
                    mycotest::Failure::Panic
                };
                writeln!(
                    &mut com1.lock(),
                    "{} {} {} {}",
                    mycotest::FAIL_TEST,
                    failure.as_str(),
                    test.module,
                    test.name
                )
                .expect("serial write failed");
            }
        }
        qemu_exit(QemuExitCode::Failed);
    }

    #[cfg(not(test))]
    unsafe {
        cpu::halt();
    }
}

#[cfg(test)]
static CURRENT_TEST: AtomicPtr<mycelium_util::testing::Test> = AtomicPtr::new(ptr::null_mut());

// TODO(eliza): this is now in arch because it uses the serial port, would be
// nice if that was cross platform...
#[cfg(test)]
pub fn run_tests() {
    use core::fmt::Write;
    let span = tracing::info_span!("run tests");
    let _enter = span.enter();

    let mut passed = 0;
    let mut failed = 0;
    let com1 = serial::com1().expect("if we're running tests, there ought to be a serial port");
    let tests = mycelium_util::testing::all_tests();
    writeln!(&mut com1.lock(), "{}{}", mycotest::TEST_COUNT, tests.len())
        .expect("serial write failed");
    for test in tests {
        CURRENT_TEST.store(test as *const _ as *mut _, Ordering::Release);

        writeln!(
            &mut com1.lock(),
            "{}{} {}",
            mycotest::START_TEST,
            test.module,
            test.name
        )
        .expect("serial write failed");

        let span = tracing::info_span!("test", test.name, test.module);
        let _enter = span.enter();

        if (test.run)() {
            writeln!(
                &mut com1.lock(),
                "{}{} {}",
                mycotest::PASS_TEST,
                test.module,
                test.name
            )
            .expect("serial write failed");
            passed += 1;
        } else {
            writeln!(
                &mut com1.lock(),
                "{}{} {}",
                mycotest::FAIL_TEST,
                test.module,
                test.name
            )
            .expect("serial write failed");
            failed += 1;
        }
    }

    CURRENT_TEST.store(ptr::null_mut(), Ordering::Release);

    tracing::warn!("{} passed | {} failed", passed, failed);
    if failed == 0 {
        qemu_exit(QemuExitCode::Success);
    } else {
        qemu_exit(QemuExitCode::Failed);
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

mycelium_util::decl_test! {
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

mycelium_util::decl_test! {
    fn alloc_some_4k_pages() -> Result<(), hal_core::mem::page::AllocErr> {
        use hal_core::mem::page::Alloc;
        let page1 = tracing::info_span!("alloc page 1").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        let page2 = tracing::info_span!("alloc page 2").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page1, page2);
        tracing::info_span!("dealloc page 1").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page1);
            tracing::info!(?res, "deallocated page 1");
            res
        })?;
        let page3 = tracing::info_span!("alloc page 3").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page2, page3);
        tracing::info_span!("dealloc page 2").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page2);
            tracing::info!(?res, "deallocated page 2");
            res
        })?;
        let page4 = tracing::info_span!("alloc page 4").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page3, page4);
        tracing::info_span!("dealloc page 3").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page3);
            tracing::info!(?res, "deallocated page 3");
            res
        })?;
        tracing::info_span!("dealloc page 4").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page4);
            tracing::info!(?res, "deallocated page 4");
            res
        })
    }
}

mycelium_util::decl_test! {
    fn alloc_4k_pages_and_ranges() -> Result<(), hal_core::mem::page::AllocErr> {
        use hal_core::mem::page::Alloc;
        let range1 = tracing::info_span!("alloc range 1").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc_range(mm::size::Size4Kb, 16);
            tracing::info!(?res);
            res
        })?;
        let page2 = tracing::info_span!("alloc page 2").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        tracing::info_span!("dealloc range 1").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc_range(range1);
            tracing::info!(?res, "deallocated range 1");
            res
        })?;
        let range3 = tracing::info_span!("alloc range 3").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc_range(mm::size::Size4Kb, 10);
            tracing::info!(?res);
            res
        })?;
        tracing::info_span!("dealloc page 2").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page2);
            tracing::info!(?res, "deallocated page 2");
            res
        })?;
        let range4 = tracing::info_span!("alloc range 4").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc_range(mm::size::Size4Kb, 8);
            tracing::info!(?res);
            res
        })?;
        tracing::info_span!("dealloc range 3").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc_range(range3);
            tracing::info!(?res, "deallocated range 3");
            res
        })?;
        tracing::info_span!("dealloc range 4").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc_range(range4);
            tracing::info!(?res, "deallocated range 4");
            res
        })
    }
}

mycelium_util::decl_test! {
    fn alloc_some_pages() -> Result<(), hal_core::mem::page::AllocErr> {
        use hal_core::mem::page::Alloc;
        let page1 = tracing::info_span!("alloc page 1").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        let page2 = tracing::info_span!("alloc page 2").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size4Kb);
            tracing::info!(?res);
            res
        })?;
        assert_ne!(page1, page2);
        tracing::info_span!("dealloc page 1").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page1);
            tracing::info!(?res, "deallocated page 1");
            res
        })?;
        // TODO(eliza): when 2mb pages work, test that too...
        // let page3 = tracing::info_span!("alloc page 3").in_scope(|| {
        //     let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size2Mb);
        //     tracing::info!(?res);
        //     res
        // })?;
        tracing::info_span!("dealloc page 2").in_scope(|| {
            let res = crate::PAGE_ALLOCATOR.dealloc(page2);
            tracing::info!(?res, "deallocated page 2");
            res
        })?;
        // let page4 = tracing::info_span!("alloc page 4").in_scope(|| {
        //     let res = crate::PAGE_ALLOCATOR.alloc(mm::size::Size2Mb);
        //     tracing::info!(?res);
        //     res
        // })?;
        // tracing::info_span!("dealloc page 3").in_scope(|| {
        //     let res = crate::PAGE_ALLOCATOR.dealloc(page3);
        //     tracing::info!(?res, "deallocated page 3");
        //     res
        // })?;
        // tracing::info_span!("dealloc page 4").in_scope(|| {
        //     let res = crate::PAGE_ALLOCATOR.dealloc(page4);
        //     tracing::info!(?res, "deallocated page 4");
        //     res
        // })
        Ok(())
    }
}
