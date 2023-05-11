#![no_std]
#![allow(unused_unsafe)]

#[cfg(feature = "buddy")]
pub mod buddy;

#[cfg(feature = "bump")]
pub mod bump;

#[cfg(feature = "sharded")]
pub mod sharded;