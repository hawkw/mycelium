#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
pub mod arch;

#[cfg(test)]
fn main() {
    unreachable!()
}
