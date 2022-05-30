#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(docsrs, doc(cfg_hide(docsrs)))]
#[cfg(feature = "alloc")]
extern crate alloc;

#[macro_use]
pub(crate) mod util;
pub(crate) mod loom;

pub mod scheduler;
pub mod task;
pub mod wait;
