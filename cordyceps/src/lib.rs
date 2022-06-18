#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(docsrs, deny(missing_docs))]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(test)]
extern crate std;

#[macro_use]
pub(crate) mod util;

pub mod list;
pub mod mpsc_queue;
pub mod transfer_stack;

#[doc(inline)]
pub use list::List;
#[doc(inline)]
pub use mpsc_queue::MpscQueue;

pub(crate) mod loom;

use core::ptr::NonNull;

/// Trait implemented by types which can be members of an [intrusive collection].
///
/// In order to be part of an intrusive collection, a type must contain a
/// `Links` type that stores the pointers to other nodes in that collection. For
/// example, to be part of a [doubly-linked list], a type must contain the
/// [`list::Links`] struct, or to be part of a [MPSC queue], a type must contain
/// the [`mpsc_queue::Links`] struct.
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
/// [intrusive collection]: crate#intrusive-data-structures
/// [`Unpin`]: core::marker::Unpin
/// [doubly-linked list]: crate::list
/// [MSPC queue]: crate::mpsc_queue
pub unsafe trait Linked<L> {
    /// The handle owning nodes in the linked list.
    ///
    /// This type must have ownership over a `Self`-typed value. When a `Handle`
    /// is dropped, it should drop the corresponding `Linked` type.
    ///
    /// A quintessential example of a `Handle` is [`Box`].
    ///
    /// [`Box`]: alloc::boxed::Box
    type Handle;

    /// Convert a [`Self::Handle`] to a raw pointer to `Self`, taking ownership
    /// of it in the process.
    fn into_ptr(r: Self::Handle) -> NonNull<Self>;

    /// Convert a raw pointer to `Self` into an owning [`Self::Handle`].
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a [`Self::Handle`] from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle;

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a [`Self::Handle`] from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<L>;
}
