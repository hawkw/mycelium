#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(docsrs, loom)))]
#![cfg_attr(not(test), no_std)]
#![allow(unused_unsafe)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[macro_use]
pub(crate) mod util;
#[macro_use]
pub(crate) mod trace;
pub(crate) mod loom;

pub mod future;
pub mod scheduler;
pub mod sync;
pub mod task;
pub mod time;
