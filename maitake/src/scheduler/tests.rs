use super::*;
use crate::{future, sync::wait_cell::test_util::Chan};

#[cfg(all(feature = "alloc", not(loom)))]
mod alloc_tests;

#[cfg(not(loom))]
mod custom_storage;

#[cfg(any(loom, feature = "alloc"))]
mod loom;
