#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]

extern crate alloc;

// Force `mycelium_alloc` to be linked in, as it provides the allocator.
#[cfg(target_os = "none")]
extern crate mycelium_alloc;

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

    A::init_interrupts(bootinfo);

    {
        // Let's allocate something, for funsies
        let mut v = Vec::new();
        writeln!(&mut writer, "vec: {:?} (@ {:p})", v, v.as_ptr()).unwrap();
        v.push(5u64);
        writeln!(&mut writer, "vec: {:?} (@ {:p})", v, v.as_ptr()).unwrap();
        v.push(10u64);
        writeln!(&mut writer, "vec: {:?} (@ {:p})", v, v.as_ptr()).unwrap();
        assert_eq!(v.pop(), Some(10));
        assert_eq!(v.pop(), Some(5));
    }

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

pub fn handle_alloc_error(writer: &mut impl Write, layout: core::alloc::Layout) -> ! {
    let _ = writeln!(writer, "alloc error:\n{:?}", layout);

    #[allow(clippy::empty_loop)]
    loop {}
}
