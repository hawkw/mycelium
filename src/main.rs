#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![cfg_attr(target_os = "none", feature(asm))]

pub mod arch;

#[panic_handler]
#[cfg(target_os = "none")]
fn panic(panic: &core::panic::PanicInfo) -> ! {
    tracing::error!(%panic);
    arch::oops(panic)
}

fn main() {
    unsafe {
        core::hint::unreachable_unchecked();
    }
}
