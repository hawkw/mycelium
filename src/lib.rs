#![cfg_attr(target_os = "none", no_std)]
use core::fmt::Write;
use hal_core::{boot::BootInfo, Architecture};

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
    loop {}
}

pub fn handle_panic(writer: &mut impl Write, info: &core::panic::PanicInfo) -> ! {
    writeln!(writer, "something went very wrong:\n{}", info).unwrap();
    loop {}
}
