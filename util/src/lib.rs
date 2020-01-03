//! A "standard library" for programming in the Mycelium kernel and related
//! libraries.
#![cfg_attr(target_os = "none", no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use mycelium_util_macros::*;

pub mod cell;
pub mod error;
pub mod io;
pub mod sync;
pub mod testing;
