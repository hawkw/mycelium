//! The simplest thing resembling an "allocator" I could possibly create.
//! Allocates into a "large" static array.
#![no_std]
#![allow(unused_unsafe)]

#[cfg(feature = "buddy")]
pub mod buddy;

#[cfg(feature = "bump")]
pub mod bump;
