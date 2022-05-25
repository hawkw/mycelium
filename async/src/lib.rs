#![cfg_attr(not(test), no_std)]
extern crate alloc;

#[macro_use]
pub(crate) mod util;
pub(crate) mod loom;

pub mod scheduler;
pub mod task;
pub mod wait;
