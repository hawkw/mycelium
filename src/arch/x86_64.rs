use hal_core::{boot::BootInfo, Architecture};
use hal_x86_64::X64;

static HELLO: &[u8] = b"Hello World!";

#[derive(Debug)]
pub struct X64BootInfo {
    _p: (),
}

impl BootInfo for X64BootInfo {
    type Arch = X64;
    // TODO(eliza): implement
    type MemoryMap = core::iter::Empty<hal_core::mem::Region<X64>>;

    /// Returns the boot info's memory map.
    fn memory_map(&self) -> Self::MemoryMap {
        core::iter::empty()
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let vga_buffer = 0xb8000 as *mut u8;

    for (i, &byte) in HELLO.iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;
            *vga_buffer.offset(i as isize * 2 + 1) = 0xb;
        }
    }
    let bootinfo = X64BootInfo { _p: () };
    mycelium_kernel::kernel_main(&bootinfo);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    mycelium_kernel::handle_panic(info)
}
