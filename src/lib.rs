#![cfg_attr(target_os = "none", no_std)]

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem, Architecture};
mod interrupt;

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

    if let Some(logger) = bootinfo.logger() {
        log::set_logger(logger).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    }

    let mut regions = 0;
    let mut free_regions = 0;
    let mut free_bytes = 0;

    log::info!("memory map:");

    for region in bootinfo.memory_map() {
        let kind = region.kind();
        let size = region.size();
        log::info!(
            "  {:>10?} {:>15?} {:>15?} B",
            region.base_addr(),
            kind,
            size,
        );
        regions += 1;
        if region.kind() == mem::RegionKind::FREE {
            free_regions += 1;
            free_bytes += size;
        }
    }

    log::info!(
        "found {} memory regions, {} free regions ({} bytes)",
        regions,
        free_regions,
        free_bytes,
    );

    A::init_interrupts::<interrupt::Handlers<A>, _>(bootinfo);

    // if this function returns we would boot loop. Hang, instead, so the debug
    // output can be read.
    //
    // eventually we'll call into a kernel main loop here...
    #[allow(clippy::empty_loop)]
    loop {}
}

pub fn handle_panic(writer: &mut impl Write, info: &core::panic::PanicInfo) -> ! {
    writeln!(writer, "something went very wrong:\n{}", info).unwrap();

    // we can't panic or make the thread sleep here, as we are in the panic
    // handler!
    #[allow(clippy::empty_loop)]
    loop {}
}
