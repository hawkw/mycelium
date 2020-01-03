#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![cfg_attr(target_os = "none", feature(asm))]
#![cfg_attr(target_os = "none", feature(panic_info_message, track_caller))]
#![cfg_attr(target_os = "none", feature(custom_test_frameworks))]
#![cfg_attr(target_os = "none", test_runner(test_runner))]
#![cfg_attr(target_os = "none", reexport_test_harness_main = "test_main")]

extern crate alloc;

use core::fmt::Write;
use hal_core::{boot::BootInfo, mem, Architecture};
use alloc::vec::Vec;

mod wasm;
mod arch;

const HELLOWORLD_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/helloworld.wasm"));

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

    #[cfg(test)]
    test_main();

    // if this function returns we would boot loop. Hang, instead, so the debug
    // output can be read.
    //
    // eventually we'll call into a kernel main loop here...
    #[allow(clippy::empty_loop)]
    loop {}
}

#[cfg(test)]
fn test_runner(tests: &[&mycelium_util::testing::TestCase]) {
    let span = tracing::info_span!("==== running tests ====", count = tests.len());
    let _enter = span.enter();

    let mut fails = 0;
    for test in tests {
        let span = tracing::info_span!("running test", test.name);
        let _enter = span.enter();
        match (test.func)() {
            Ok(_) => tracing::info!(test.name, "TEST OK"),
            Err(_) => {
                fails += 1;
                tracing::error!(test.name, "TEST FAIL");
            }
        }
    }

    let exit_code;
    if fails == 0 {
        tracing::info!("Tests OK");
        exit_code = arch::QemuExitCode::Success;
    } else {
        tracing::error!(fails, "Some Tests FAILED");
        exit_code = arch::QemuExitCode::Failed;
    }

    arch::qemu_exit(exit_code);
    panic!("failed to exit qemu");
}

#[global_allocator]
pub static GLOBAL: mycelium_alloc::Alloc = mycelium_alloc::Alloc;

#[alloc_error_handler]
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
    arch::oops(&pp)
}

#[mycelium_util::test]
fn test_alloc() {
    // Let's allocate something, for funsies
    let mut v = Vec::new();
    tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
    v.push(5u64);
    tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
    v.push(10u64);
    tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
    assert_eq!(v.pop(), Some(10));
    assert_eq!(v.pop(), Some(5));
}


#[mycelium_util::test]
fn test_wasm() {
    match wasm::run_wasm(HELLOWORLD_WASM) {
        Ok(()) => tracing::info!("wasm test Ok!"),
        Err(err) => tracing::error!(?err, "wasm test Err"),
    }
}

fn main() {
    unsafe {
        core::hint::unreachable_unchecked();
    }
}
