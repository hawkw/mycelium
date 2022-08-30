#[cfg(any(loom, feature = "alloc"))]
use super::*;

#[cfg(loom)]
mod loom;

#[cfg(all(not(loom), feature = "alloc"))]
mod alloc;
