#![cfg_attr(all(target_os = "none", test), no_main)]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![cfg_attr(target_os = "none", feature(panic_info_message))]

extern crate alloc;
extern crate rlibc;

pub mod arch;

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem};
use mycelium_alloc::buddy;

mod wasm;

static PAGE_ALLOCATOR: buddy::Alloc = buddy::Alloc::new_default(arch::mm::MIN_PAGE_SIZE);

pub fn kernel_main(bootinfo: &impl BootInfo) -> ! {
    let mut writer = bootinfo.writer();
    writeln!(
        &mut writer,
        "hello from mycelium {} (on {})",
        env!("CARGO_PKG_VERSION"),
        arch::NAME
    )
    .unwrap();
    writeln!(&mut writer, "booting via {}", bootinfo.bootloader_name()).unwrap();

    if let Some(subscriber) = bootinfo.subscriber() {
        tracing::dispatcher::set_global_default(subscriber).unwrap();
    }

    arch::interrupt::init::<arch::InterruptHandlers>();
    bootinfo.init_paging();

    // XXX(eliza): this sucks
    PAGE_ALLOCATOR.set_vm_offset(arch::mm::vm_offset());

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
                unsafe {
                    tracing::trace!(?region, "adding to page allocator");
                    let e = PAGE_ALLOCATOR.add_region(region);
                    tracing::trace!(added = e.is_ok());
                }
            }
        }

        tracing::info!(
            "found {} memory regions, {} free regions ({} bytes)",
            regions,
            free_regions,
            free_bytes,
        );
    }

    #[cfg(test)]
    arch::run_tests();

    // if this function returns we would boot loop. Hang, instead, so the debug
    // output can be read.
    //
    // eventually we'll call into a kernel main loop here...
    #[allow(clippy::empty_loop)]
    #[allow(unreachable_code)]
    loop {}
}

mycelium_util::decl_test! {
    fn basic_alloc() {
        // Let's allocate something, for funsies
        use alloc::vec::Vec;
        let mut v = Vec::new();
        tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
        v.push(5u64);
        tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
        v.push(10u64);
        tracing::info!(vec=?v, vec.addr=?v.as_ptr());
        assert_eq!(v.pop(), Some(10));
        assert_eq!(v.pop(), Some(5));
    }
}

mycelium_util::decl_test! {
    fn wasm_hello_world() -> Result<(), wasmi::Error> {
        const HELLOWORLD_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/helloworld.wasm"));
        wasm::run_wasm(HELLOWORLD_WASM)
    }
}

#[global_allocator]
#[cfg(target_os = "none")]
pub static GLOBAL: mycelium_alloc::bump::Alloc = mycelium_alloc::bump::Alloc;

#[alloc_error_handler]
#[cfg(target_os = "none")]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("alloc error: {:?}", layout);
}

#[cfg(target_os = "none")]
#[panic_handler]
#[cold]
fn panic(panic: &core::panic::PanicInfo) -> ! {
    use core::fmt;

    struct PrettyPanic<'a>(&'a core::panic::PanicInfo<'a>);
    impl<'a> fmt::Display for PrettyPanic<'a> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let message = self.0.message();
            let location = self.0.location();
            let caller = core::panic::Location::caller();
            if let Some(message) = message {
                writeln!(f, "  mycelium panicked: {}", message)?;
                if let Some(loc) = location {
                    writeln!(f, "  at: {}:{}:{}", loc.file(), loc.line(), loc.column(),)?;
                }
            } else {
                writeln!(f, "  mycelium panicked: {}", self.0)?;
            }
            writeln!(
                f,
                "  in: {}:{}:{}",
                caller.file(),
                caller.line(),
                caller.column()
            )?;
            Ok(())
        }
    }

    let caller = core::panic::Location::caller();
    tracing::error!(%panic, ?caller);
    let pp = PrettyPanic(panic);
    arch::oops(&pp, None)
}

#[cfg(all(test, not(target_os = "none")))]
pub fn main() {
    /* no host-platform tests in this crate */
}
