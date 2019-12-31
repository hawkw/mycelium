//! The Mycelium I/O Prelude
//!
//! The purpose of this module is to alleviate imports of many common I/O traits
//! by adding a glob import to the top of I/O heavy modules:
//!
//! ```
//! # #![allow(unused_imports)]
//! use mycelium_util::io::prelude::*;
//! ```
pub use super::{BufRead, Read, Seek, Write};
