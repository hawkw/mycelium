use super::*;
use crate::loom::sync::atomic::{AtomicUsize, Ordering};
use crate::{future, wait::cell::test_util::Chan};

#[cfg(all(feature = "alloc", not(loom)))]
mod alloc;

#[cfg(not(loom))]
mod custom_storage;

#[cfg(loom)]
mod loom;
