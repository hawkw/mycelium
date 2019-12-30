#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
pub mod arch;

fn main() {
    unsafe {
        core::hint::unreachable_unchecked();
    }
}
