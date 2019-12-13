//! A "standard library" for programming in the Mycelium kernel and related
//! libraries.
#![cfg_attr(target_os = "none", no_std)]

pub mod cell;
pub mod error;
pub mod sync;
