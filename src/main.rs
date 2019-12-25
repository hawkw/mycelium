#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
pub mod arch;

fn main() {
    unsafe {
        core::hint::unreachable_unchecked();
    }
}

#[cfg_attr(target_os = "none", alloc_error_handler)]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("bad news: failed to allocate {:?}", layout)
}
