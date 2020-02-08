#![no_main]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(alloc_error_handler))]
#![cfg_attr(target_os = "none", feature(asm))]
#![cfg_attr(target_os = "none", feature(panic_info_message, track_caller))]

// Force linking to the `mycelium_kernel` lib.
#[allow(unused_imports)]
#[allow(clippy::single_component_path_imports)]
use mycelium_kernel;
