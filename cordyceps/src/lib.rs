//! Mycelium intrusive data structures.
//!
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(feature = "alloc")]
extern crate alloc;
pub mod list;
pub use list::List;
pub mod mpsc_queue;
pub use mpsc_queue::Queue;

pub(crate) mod loom;
pub(crate) mod util;

use core::ptr::NonNull;
/// Trait implemented by types which can be members of an intrusive collection.
///
/// # Safety
///
/// This is unsafe to implement because it's the implementation's responsibility
/// to ensure that the `Node` associated type is a valid linked list node. In
/// particular:
///
/// - Implementations must ensure that `Node`s are pinned in memory while they
///   are in a linked list. While a given `Node` is in a linked list, it may not
///   be deallocated or moved to a different memory location.
/// - The `Node` type may not be [`Unpin`].
/// - Additional safety requirements for individual methods on this trait are
///   documented on those methods.
///
/// Failure to uphold these invariants will result in list corruption, including
/// dangling pointers.
///
/// [`Unpin`]: core::pin::Unpin
pub unsafe trait Linked<L> {
    /// The handle owning nodes in the linked list.
    type Handle;

    /// Convert a `Handle` to a raw pointer, without consuming it.
    #[allow(clippy::wrong_self_convention)]
    fn as_ptr(r: &Self::Handle) -> NonNull<Self>;

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle;

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<L>;
}
