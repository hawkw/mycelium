#![cfg_attr(target_os = "none", no_std)]
use core::fmt::Write;
use hal_core::{boot::BootInfo, mem, Architecture};

pub fn kernel_main<A>(bootinfo: &impl BootInfo<Arch = A>) -> !
where
    A: Architecture,
{
    let mut writer = bootinfo.writer();
    writeln!(
        &mut writer,
        "hello from mycelium {} (on {})",
        env!("CARGO_PKG_VERSION"),
        A::NAME
    )
    .unwrap();
    writeln!(&mut writer, "booting via {}", bootinfo.bootloader_name()).unwrap();

    let mut regions = 0;
    let mut free_regions = 0;
    let mut free_bytes = 0;

    writeln!(&mut writer, "memory map:").unwrap();

    for region in bootinfo.memory_map() {
        let kind = region.kind();
        let size = region.size();
        writeln!(
            &mut writer,
            "  {:>10?} {:>15?} {:>15?} B",
            region.base_addr(),
            kind,
            size,
        )
        .unwrap();
        regions += 1;
        if region.kind() == mem::RegionKind::FREE {
            free_regions += 1;
            free_bytes += size;
        }
    }

    writeln!(
        &mut writer,
        "found {} memory regions, {} free regions ({} bytes)",
        regions, free_regions, free_bytes,
    )
    .unwrap();
    loop {}
}

pub fn handle_panic(writer: &mut impl Write, info: &core::panic::PanicInfo) -> ! {
    writeln!(writer, "something went very wrong:\n{}", info).unwrap();
    loop {}
}
