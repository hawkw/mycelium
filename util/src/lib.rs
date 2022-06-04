#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(target_os = "none", no_std)]
#![allow(unused_unsafe)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#[cfg(feature = "alloc")]
extern crate alloc;

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
pub use mycelium_bitfield as bits;
