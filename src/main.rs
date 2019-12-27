#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
pub mod arch;

unsafe fn main() {
    core::hint::unreachable_unchecked();
}
