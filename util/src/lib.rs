#![doc = include_str!("../README.md")]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![allow(unused_unsafe)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[macro_use]
mod macros;

pub mod error;
pub mod fmt;
pub mod io;
pub mod math;
pub mod sync;

pub(crate) mod loom;

pub use self::macros::*;
pub use cordyceps as intrusive;
pub use mycelium_bitfield as bits;
