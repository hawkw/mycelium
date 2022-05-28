//! A "standard library" for programming in the Mycelium kernel and related
//! libraries.
#![cfg_attr(target_os = "none", no_std)]
#![allow(unused_unsafe)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![feature(trace_macros)]
#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bits;
pub mod cell;
pub mod error;
pub mod fmt;
pub mod io;
mod macros;
pub mod math;
pub mod sync;

pub(crate) mod loom;

pub use self::macros::*;
pub use cordyceps as intrusive;
