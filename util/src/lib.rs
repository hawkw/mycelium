//! A "standard library" for programming in the Mycelium kernel and related
//! libraries.
#![cfg_attr(target_os = "none", no_std)]
#![feature(
    const_fn_trait_bound // To allow trait bounds on const fn constructors.
)]
#![allow(unused_unsafe)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]

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
pub mod testing;

pub(crate) mod loom;

pub use self::macros::*;
pub use cordyceps as intrusive;
