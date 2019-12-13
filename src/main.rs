#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
pub mod arch;

#[cfg(not(target_os = "none"))]
fn main() {
    unreachable!()
}
