#![doc = include_str!("../README.md")]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![allow(unused_unsafe)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[macro_use]
mod macros;

pub use deferred::defer;
pub mod deferred;
pub mod error;
pub mod fmt;
pub mod io;
pub mod math;
pub mod mem;
pub mod sync;

pub(crate) mod loom;

pub use cordyceps as intrusive;
pub use mycelium_bitfield as bits;

#[cfg(test)]
pub(crate) mod test_util {
    #[cfg(not(loom))]
    pub(crate) fn trace_init() -> impl Drop {
        use tracing_subscriber::{prelude::*, EnvFilter};
        let filter = EnvFilter::from_env("RUST_LOG");
        tracing_subscriber::fmt()
            .with_test_writer()
            .without_time()
            .with_env_filter(filter)
            .with_thread_names(true)
            .set_default()
    }

    #[cfg(loom)]
    pub(crate) fn trace_init() -> impl Drop {
        use tracing_subscriber_03::{prelude::*, EnvFilter};
        let filter = EnvFilter::from_env("LOOM_LOG");
        tracing_subscriber_03::fmt()
            .with_test_writer()
            .without_time()
            .with_env_filter(filter)
            .set_default()
    }
}
