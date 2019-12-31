#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]

extern crate alloc;

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem, Architecture};

use alloc::vec::Vec;

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

    if let Some(subscriber) = bootinfo.subscriber() {
        tracing::dispatcher::set_global_default(subscriber).unwrap();
    }

    let mut regions = 0;
    let mut free_regions = 0;
    let mut free_bytes = 0;

    {
        let span = tracing::info_span!("memory map");
        let _enter = span.enter();
        for region in bootinfo.memory_map() {
            let kind = region.kind();
            let size = region.size();
            tracing::info!(
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

        tracing::info!(
            "found {} memory regions, {} free regions ({} bytes)",
            regions,
            free_regions,
            free_bytes,
        );
    }

    A::init_interrupts(bootinfo);

    {
        let span = tracing::info_span!("alloc test");
        let _enter = span.enter();
        // Let's allocate something, for funsies
        let mut v = Vec::new();
        tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
        v.push(5u64);
        tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
        v.push(10u64);
        tracing::info!(vec=?v, vec.addr=?v.as_ptr());
        assert_eq!(v.pop(), Some(10));
        assert_eq!(v.pop(), Some(5));
    }

    panic!("fake panic!");

    // if this function returns we would boot loop. Hang, instead, so the debug
    // output can be read.
    //
    // eventually we'll call into a kernel main loop here...
    #[allow(clippy::empty_loop)]
    loop {}
}

#[global_allocator]
#[cfg(target_os = "none")]
pub static GLOBAL: mycelium_alloc::Alloc = mycelium_alloc::Alloc;

#[alloc_error_handler]
#[cfg(target_os = "none")]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("alloc error: {:?}", layout);
}
