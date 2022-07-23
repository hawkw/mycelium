use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use maitake::{future, scheduler::*};

#[path = "../util.rs"]
mod util;

#[cfg(feature = "alloc")]
mod alloc;
mod custom_storage;
