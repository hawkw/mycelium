//! A "standard library" for programming in the Mycelium kernel and related
//! libraries.
#![cfg_attr(target_os = "none", no_std)]
#![allow(unused_unsafe)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod cell;
pub mod error;
pub mod intrusive;
pub mod io;
mod macros;
pub mod sync;
pub mod testing;
pub mod trace;

pub use self::macros::*;
