#![cfg_attr(target_os = "none", no_std)]
extern crate alloc;

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem, Address, Architecture};

mod interrupt;

#[global_allocator]
#[cfg(target_os = "none")]
static ALLOCATOR: mycelium_alloc::LockAlloc = mycelium_alloc::LockAlloc::none();

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
    let mut largest_free: Option<mem::Region<_>> = None;

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
            if region.base_addr().is_page_aligned::<A::MinPageSize>() {
                write!(&mut writer, ", page-aligned").unwrap();
                // if let Some(max) = largest_free.as_ref() {
                //     if size > max.size() {
                //         largest_free = Some(region);
                //     }
                // } else {
                //     largest_free = Some(region);
                // }
                if largest_free.is_none() {
                    largest_free = Some(region);
                }
            }
        }
        writeln!(&mut writer, "").unwrap();
    }

    log::info!(
        "found {} memory regions, {} free regions ({} bytes)",
        regions,
        free_regions,
        free_bytes,
    );

    A::init_interrupts::<interrupt::Handlers<A>, _>(bootinfo);

    let largest_free = largest_free.expect("found no free memory regions!");
    log::info!("largest free memory region: {:?}", largest_free,);

    let bump_pg = largest_free
        .page_range::<A::MinPageSize>()
        .expect("region was already checked for alignment")
        .next()
        .unwrap();

    log::debug!("bump allocator page {:?}", bump_pg);
    let bump = mycelium_alloc::bump::BumpPage::new(bump_pg);
    ALLOCATOR.set_allocator(bump.as_dyn_alloc());

    let a_vec = alloc::vec![1, 2, 3];
    log::info!("allocated a vec: {:?}", a_vec);

    loop {}
}

pub fn handle_panic(writer: &mut impl Write, info: &core::panic::PanicInfo) -> ! {
    writeln!(writer, "\nsomething went very wrong:\n{}", info).unwrap();
    loop {}
}
