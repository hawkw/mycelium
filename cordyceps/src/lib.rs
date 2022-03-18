#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#[cfg(feature = "alloc")]
extern crate alloc;
pub mod list;
pub use list::List;
pub mod mpsc_queue;
pub use mpsc_queue::MpscQueue;

pub(crate) mod loom;
pub(crate) mod util;

use core::ptr::NonNull;
/// Trait implemented by types which can be members of an intrusive collection.
///
/// # Safety
///
/// This is unsafe to implement because it's the implementation's responsibility
/// to ensure that types implementing this trait are valid intrusive collection
/// nodes. In particular:
///
/// - Implementations **must** ensure that implementorss are pinned in memory while they
///   are in an intrusive collection. While a given `Linked` type is in an intrusive
///   data structure, it may not be deallocated or moved to a different memory
///   location.
/// - The type implementing this trait **must not** implement [`Unpin`].
/// - Additional safety requirements for individual methods on this trait are
///   documented on those methods.
///
/// Failure to uphold these invariants will result in corruption of the
/// intrusive data structure, including dangling pointers.
///
/// [`Unpin`]: core::pin::Unpin
pub unsafe trait Linked<L> {
    /// The handle owning nodes in the linked list.
    type Handle;

    /// Convert a `Handle` to a raw pointer.
    fn as_ptr(r: Self::Handle) -> NonNull<Self>;

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
