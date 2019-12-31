#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]

pub mod arch;

fn main() {
    unsafe {
        core::hint::unreachable_unchecked();
    }
}
