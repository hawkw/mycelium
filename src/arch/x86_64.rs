use hal_core::boot::BootInfo;
use hal_x86_64::{vga, X64};

#[derive(Debug)]
pub struct X64BootInfo {
    _p: (),
}

impl BootInfo for X64BootInfo {
    type Arch = X64;
    // TODO(eliza): implement
    type MemoryMap = core::iter::Empty<hal_core::mem::Region<X64>>;

    type Writer = vga::Writer;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap {
        unimplemented!("eliza: add this!")
    }

    fn writer(&self) -> Self::Writer {
        vga::writer()
    }
}

#[no_mangle]
#[cfg(not(test))]
pub extern "C" fn _start() -> ! {
    // TODO(eliza): unpack bootinfo!
    let bootinfo = X64BootInfo { _p: () };
    mycelium_kernel::kernel_main(&bootinfo);
}

#[panic_handler]
#[cfg(not(test))]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut vga = vga::writer();
    vga.set_color(vga::ColorSpec::new(vga::Color::Red, vga::Color::Black));
    mycelium_kernel::handle_panic(&mut vga, info)
}
