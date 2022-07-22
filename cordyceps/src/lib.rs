#![cfg_attr(docsrs, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg, doc_cfg_hide))]
#![cfg_attr(docsrs, deny(missing_docs))]
#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![allow(unused_unsafe)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(test)]
extern crate std;

macro_rules! feature {
    (
        #![$meta:meta]
        $($item:item)*
    ) => {
        $(
            #[cfg($meta)]
            $item
        )*
    }
}

pub mod list;
#[doc(inline)]
pub use list::List;
pub mod mpsc_queue;
#[doc(inline)]
pub use mpsc_queue::MpscQueue;

pub(crate) mod loom;
pub(crate) mod util;

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
/// # Implementing `Linked::links`
///
/// The [`Linked::links`] method provides access to a `Linked` type's `Links`
/// field through a [`NonNull`] pointer. This is necessary for a type to
/// participate in an intrusive structure, as it tells the intrusive structure
/// how to access the links to other parts of that data structure. However, this
/// method is somewhat difficult to implement correctly.
///
/// Suppose we have an entry type like this:
/// ```rust
/// use cordyceps::list;
///
/// struct Entry {
///     links: list::Links<Self>,
///     data: usize,
/// }
/// ```
///
/// The naive implementation of [`links`](Linked::links) for this `Entry` type
/// might look like this:
///
/// ```
/// use cordyceps::Linked;
/// use core::ptr::NonNull;
///
/// # use cordyceps::list;
/// # struct Entry {
/// #    links: list::Links<Self>,
/// # }
///
/// unsafe impl Linked<list::Links<Self>> for Entry {
///     # type Handle = NonNull<Self>;
///     # fn into_ptr(r: Self::Handle) -> NonNull<Self> { r }
///     # unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }
///     // ...
///
///     unsafe fn links(mut target: NonNull<Self>) -> NonNull<list::Links<Self>> {
///         // Borrow the target's `links` field.
///         let links = &mut target.as_mut().links;
///         // Convert that reference into a pointer.
///         NonNull::from(links)
///     }
/// }
/// ```
///
/// However, this implementation **is not sound** under [Stacked Borrows]! It
/// creates a temporary reference from the original raw pointer, and then
/// creates a new raw pointer from that temporary reference. Stacked Borrows
/// will reject this reborrow as unsound.[^1]
///
/// There are two ways we can implement [`Linked::links`] without creating a
/// temporary reference in this manner. The recommended one is to use the
/// [`core::ptr::addr_of_mut!`] macro, as follows:
///
/// ```
/// use core::ptr::{self, NonNull};
/// # use cordyceps::{Linked, list};
/// # struct Entry {
/// #    links: list::Links<Self>,
/// # }
///
/// unsafe impl Linked<list::Links<Self>> for Entry {
///     # type Handle = NonNull<Self>;
///     # fn into_ptr(r: Self::Handle) -> NonNull<Self> { r }
///     # unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }
///     // ...
///
///     unsafe fn links(target: NonNull<Self>) -> NonNull<list::Links<Self>> {
///         let target = target.as_ptr();
///
///         // Using the `ptr::addr_of_mut!` macro, we can offset a raw pointer to a
///         // raw pointer to a field *without* creating a temporary reference.
///         let links = ptr::addr_of_mut!((*target).links);
///
///         // `NonNull::new_unchecked` is safe to use here, because the pointer that
///         // we offset was not null, implying that the pointer produced by offsetting
///         // it will also not be null.
///         NonNull::new_unchecked(links)
///     }
/// }
/// ```
///
/// It is also possible to ensure that the struct implementing `Linked` is laid
/// out so that the `Links` field is the first member of the struct, and then
/// cast the pointer to a `Links`. Since [Rust's native type representation][repr]
/// does not guarantee the layout of struct members, it is **necessary** to ensure
/// that any struct that implements the `Linked::links` method in this manner has a
/// [`#[repr(C)]` attribute][repr-c], ensuring that its fields are laid out in the
/// order that they are defined.
///
/// For example:
///
/// ```
/// use core::ptr::NonNull;
/// use cordyceps::{Linked, list};
///
/// // This `repr(C)` attribute is *mandatory* here, as it ensures that the
/// // `links` field will *always* be the first field in the struct's in-memory
/// // representation.
/// #[repr(C)]
/// struct Entry {
///     links: list::Links<Self>,
///     data: usize,
/// }
///
/// unsafe impl Linked<list::Links<Self>> for Entry {
///     # type Handle = NonNull<Self>;
///     # fn into_ptr(r: Self::Handle) -> NonNull<Self> { r }
///     # unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }
///     // ...
///
///     unsafe fn links(target: NonNull<Self>) -> NonNull<list::Links<Self>> {
///         // Safety: this performs a layout-dependent cast! it is only sound
///         // if the `Entry` type has a `#[repr(C)]` attribute!
///         target.cast::<list::Links<Self>>()
///     }
/// }
/// ```
///
/// In general, this approach is not recommended, and using
/// [`core::ptr::addr_of_mut!`] should be preferred in almost all cases. In
/// particular, the layout-dependent cast is more error-prone, as it requires a
/// `#[repr(C)]` attribute to avoid soundness issues. Additionally, the
/// layout-based cast does not permit a single struct to contain `Links` fields
/// for multiple intrusive data structures, as the `Links` type *must* be the
/// struct's first field.[^2] Therefore, [`Linked::links`] should generally be
/// implemented using [`addr_of_mut!`](core::ptr::addr_of_mut).
///
/// [^1]: Note that code like this is not *currently* known to result in
///     miscompiles, but it is rejected by tools like Miri as being unsound.
///     Like all undefined behavior, there is no guarantee that future Rust
///     compilers will not miscompile code like this, with disastrous results.
/// [^2]: And two different fields cannot both be the first field at the same
///      time...by definition.
///
/// [intrusive collection]: crate#intrusive-data-structures
/// [`Unpin`]: core::marker::Unpin
/// [doubly-linked list]: crate::list
/// [MSPC queue]: crate::mpsc_queue
/// [Stacked Borrows]: https://github.com/rust-lang/unsafe-code-guidelines/blob/master/wip/stacked-borrows.md
/// [repr]: https://doc.rust-lang.org/nomicon/repr-rust.html
/// [repr-c]: https://doc.rust-lang.org/nomicon/other-reprs.html#reprc
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
    ///
    /// See [the trait-level documentation](#implementing-linkedlinks) for
    /// details on how to correctly implement this method.
    unsafe fn links(ptr: NonNull<Self>) -> NonNull<L>;
}
