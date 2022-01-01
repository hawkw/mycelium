pub mod list;
pub use list::List;
pub mod mpsc_queue;

use core::ptr::NonNull;
/// Trait implemented by types which can be members of an intrusive linked list.
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
pub unsafe trait Linked {
    /// The handle owning nodes in the linked list.
    type Handle;
    type Links;
    // /// Type of nodes in the linked list.
    // ///
    // /// When the type implementing `Linked` is not itself a reference, this is
    // /// typically `Self`.
    // ///
    // /// # Safety
    // ///
    // /// This type may not be [`Unpin`].
    // ///
    // ///  [`Unpin`]: core::pin::Unpin
    // type Node: ?Sized;

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
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<Self::Links>;
}
