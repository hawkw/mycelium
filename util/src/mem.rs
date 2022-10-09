//! Basic functions for dealing with memory.
//!
//! This module re-exports all of [`core::mem`], along with additional
//! `mycelium-util` APIs:
//!
//! - [`CheckedMaybeUninit`]: a wrapper around [`core::mem::MaybeUninit`] that
//!       tracks whether the memory location is uninitialized when debug
//!       assertions are enabled.
pub use self::maybe_uninit::CheckedMaybeUninit;
pub use core::mem::*;
mod maybe_uninit;
