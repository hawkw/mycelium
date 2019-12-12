#![no_std]
use hal_core::{boot::BootInfo, Architecture};

pub fn kernel_main<A>(bootinfo: &impl BootInfo<Arch = A>) -> !
where
    A: Architecture,
{
    loop {}
}

pub fn handle_panic(_info: &core::panic::PanicInfo) -> ! {
    // TODO(eliza): do something here
    loop {}
}
