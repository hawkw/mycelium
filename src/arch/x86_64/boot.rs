//! Glue for the `bootloader` crate

use super::framebuf::{self, FramebufWriter};
use bootloader::boot_info;
use hal_core::{boot::BootInfo, mem, PAddr, VAddr};
use hal_x86_64::{mm, serial, vga};
use mycelium_util::sync::InitOnce;

#[derive(Debug)]
pub struct RustbootBootInfo {
    inner: &'static boot_info::BootInfo,
    has_framebuffer: bool,
}

#[derive(Debug)]
pub struct ArchInfo {
    pub(in crate::arch) rsdp_addr: Option<PAddr>,
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
            color::AnsiEscapes,
            embedded_graphics::MakeTextWriter,
            writer::{self, MakeWriterExt},
            Subscriber,
        };

        type FilteredFramebuf = writer::WithMaxLevel<MakeTextWriter<FramebufWriter>>;
        type FilteredSerial = writer::WithFilter<
            AnsiEscapes<&'static serial::Port>,
            fn(&tracing::Metadata<'_>) -> bool,
        >;

        static COLLECTOR: InitOnce<Subscriber<FilteredFramebuf, Option<FilteredSerial>>> =
            InitOnce::uninitialized();

        if !self.has_framebuffer {
            // TODO(eliza): we should probably write to just the serial port if
            // there's no framebuffer...
            return None;
        }

        fn serial_filter(meta: &tracing::Metadata<'_>) -> bool {
            // disable really noisy traces from maitake
            // TODO(eliza): it would be nice if this was configured by
            // non-arch-specific OS code...
            const DISABLED_TARGETS: &[&str] = &[
                "maitake::time",
                "maitake::task",
                "runtime::waker",
                "mycelium_alloc",
            ];
            DISABLED_TARGETS
                .iter()
                .all(|target| !meta.target().starts_with(target))
        }

        let collector = COLLECTOR.get_or_else(|| {
            let display_writer = MakeTextWriter::new(|| unsafe { framebuf::mk_framebuf() })
                .with_max_level(tracing::Level::INFO);
            let serial = serial::com1().map(|com1| {
                let com1 = AnsiEscapes::new(com1);
                com1.with_filter(serial_filter as for<'a, 'b> fn(&'a tracing::Metadata<'b>) -> bool)
            });
            Subscriber::display_only(display_writer).with_serial(serial)
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

    pub(super) fn from_bootloader(inner: &'static mut boot_info::BootInfo) -> (Self, ArchInfo) {
        let has_framebuffer = framebuf::init(inner);
        let archinfo = ArchInfo {
            rsdp_addr: inner.rsdp_addr.into_option().map(PAddr::from_u64),
        };
        let bootinfo = Self {
            inner,
            has_framebuffer,
        };
        (bootinfo, archinfo)
    }
}
