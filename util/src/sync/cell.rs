//! A variant of [`core::cell::UnsafeCell`] specialized for use in
//! implementations of synchronization primitives.
//!
//! When the `cfg(loom)` flag is enabled, the [`UnsafeCell`] and [`Cell`] types
//! in this module are re-exports of [`loom`]'s [checked
//! `UnsafeCell`][loom-unsafecell] and [checked `Cell`][loom-cell] types. When
//! Loom is not enabled, [`UnsafeCell`] is a wrapper around
//! [`core::cell::UnsafeCell`] that implements the [`loom::cell::UnsafeCell`
//! interface][loom-unsafecell], and `Cell` is a re-export of [`core::cell::Cell`].
//!
//! [`loom`]: https:://crates.io/crates/loom
//! [loom-unsafecell]: https://docs.rs/loom/latest/loom/cell/struct.UnsafeCell.html

pub use self::unsafe_cell::*;

#[cfg(loom)]
pub use loom::cell::Cell;

#[cfg(not(loom))]
pub use core::cell::Cell;

#[cfg(loom)]
mod unsafe_cell {
    pub use loom::cell::{ConstPtr, MutPtr, UnsafeCell};
}

#[cfg(not(loom))]
mod unsafe_cell {
    #![allow(dead_code)]
    use core::cell;

    /// A variant of [`core::cell::UnsafeCell`] that may be checked when
    /// [Loom] model checking is enabled.
    ///
    /// This type is similar to [`core::cell::UnsafeCell`], except when the
    /// `cfg(loom)` cfg flag is enabled, it is replaced with a variant that
    /// participates in [Loom] model checking. See [`loom::cell::UnsafeCell`]
    /// for details on this.
    ///
    /// When `cfg(loom)` is *not* set, this type is essentially a
    /// [`core::cell::UnsafeCell`], but with an API that matches that of the
    /// checked Loom cell.
    ///
    /// Instead of providing a `get()` API, this version of `UnsafeCell` provides
    /// [`with`] and [`with_mut`]. Both functions take a closure in order to track the
    /// start and end of the access to the underlying cell.
    ///
    /// [Loom]: https:://crates.io/crates/loom
    /// [`loom::cell::UnsafeCell`]: https://docs.rs/loom/latest/loom/cell/struct.UnsafeCell.html
    /// [`with`]: Self::with
    /// [`with_mut`]: Self::with_mut
    #[derive(Debug)]
    pub struct UnsafeCell<T> {
        data: cell::UnsafeCell<T>,
    }

    /// An immutable raw pointer to an [`UnsafeCell`] that may be checked when
    /// [Loom] model checking is enabled.
    ///
    /// This type is essentially a [`*const T`], but with the added ability to
    /// participate in Loom's [`UnsafeCell`] access tracking when the
    /// `cfg(loom)` cfg flag is set. While a `ConstPtr` to a given
    /// [`UnsafeCell`] exists, Loom will track that the [`UnsafeCell`] is
    /// being accessed immutably.
    ///
    /// When `cfg(loom)` is *not* set, this type is equivalent to a normal
    /// `*const T`.
    ///
    /// [`ConstPtr`]s are produced by the [`UnsafeCell::get`] method. The pointed
    /// value can be accessed using [`ConstPtr::deref`].
    ///
    /// Any number of [`ConstPtr`]s may concurrently access a given [`UnsafeCell`].
    /// However, if the [`UnsafeCell`] is accessed mutably (by
    /// [`UnsafeCell::with_mut`] or [`UnsafeCell::get_mut`]) while a [`ConstPtr`]
    /// exists, Loom will detect the concurrent mutable and immutable accesses and
    /// panic.
    ///
    /// Note that the cell is considered to be immutably accessed for *the entire
    /// lifespan of the `ConstPtr`*, not just when the `ConstPtr` is actively
    /// dereferenced.
    ///
    /// # Safety
    ///
    /// Although the `ConstPtr` type is checked for concurrent access violations, it
    /// is **still a raw pointer**. A `ConstPtr` is not bound to the lifetime of the
    /// [`UnsafeCell`] from which it was produced, and may outlive the cell. Loom
    /// does *not* currently check for dangling pointers. Therefore, the user is
    /// responsible for ensuring that a `ConstPtr` does not dangle. However, unlike
    /// a normal `*const T`, `ConstPtr`s may only be produced from a valid
    /// [`UnsafeCell`], and therefore can be assumed to never be null.
    ///
    /// Additionally, it is possible to write code in which raw pointers to an
    /// [`UnsafeCell`] are constructed that are *not* checked by Loom. If a raw
    /// pointer "escapes" Loom's tracking, invalid accesses may not be detected,
    /// resulting in tests passing when they should have failed. See [here] for
    /// details on how to avoid accidentally escaping the model.
    ///
    /// [`*const T`]: https://doc.rust-lang.org/stable/std/primitive.pointer.html
    /// [here]: #correct-usage
    /// [Loom]: https:://crates.io/crates/loom
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct ConstPtr<T: ?Sized>(*const T);

    /// A mutable raw pointer to an [`UnsafeCell`] that may be checked when
    /// [Loom] model checking is enabled.
    ///
    /// This type is essentially a [`*mut T`], but with the added ability to
    /// participate in Loom's [`UnsafeCell`] access tracking when the
    /// `cfg(loom)` cfg flag is set. While a `MutPtr` to a given [`UnsafeCell`]
    /// exists, Loom will track that the [`UnsafeCell`] is
    /// being accessed mutably.
    ///
    /// When `cfg(loom)` is *not* set, this type is equivalent to a normal
    /// `*mut T`.
    ///
    /// [`MutPtr`]s are produced by the [`UnsafeCell::get_mut`] method. The pointed
    /// value can be accessed using [`MutPtr::deref`].
    ///
    /// If an [`UnsafeCell`] is accessed mutably (by [`UnsafeCell::with_mut`] or
    /// [`UnsafeCell::get_mut`]) or immutably (by [`UnsafeCell::with`] or
    /// [`UnsafeCell::get`]) while a [`MutPtr`] to that cell exists, Loom will
    /// detect the invalid accesses and panic.
    ///
    /// Note that the cell is considered to be mutably accessed for *the entire
    /// lifespan of the `MutPtr`*, not just when the `MutPtr` is actively
    /// dereferenced.
    ///
    /// # Safety
    ///
    /// Although the `MutPtr` type is checked for concurrent access violations, it
    /// is **still a raw pointer**. A `MutPtr` is not bound to the lifetime of the
    /// [`UnsafeCell`] from which it was produced, and may outlive the cell. Loom
    /// does *not* currently check for dangling pointers. Therefore, the user is
    /// responsible for ensuring that a `MutPtr` does not dangle. However, unlike
    /// a normal `*mut T`, `MutPtr`s may only be produced from a valid
    /// [`UnsafeCell`], and therefore can be assumed to never be null.
    ///
    /// Additionally, it is possible to write code in which raw pointers to an
    /// [`UnsafeCell`] are constructed that are *not* checked by Loom. If a raw
    /// pointer "escapes" Loom's tracking, invalid accesses may not be detected,
    /// resulting in tests passing when they should have failed. See [here] for
    /// details on how to avoid accidentally escaping the model.
    ///
    /// [`*mut T`]: https://doc.rust-lang.org/stable/std/primitive.pointer.html
    /// [here]: #correct-usage
    /// [Loom]: https:://crates.io/crates/loom
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct MutPtr<T: ?Sized>(*mut T);

    impl<T> UnsafeCell<T> {
        /// Construct a new instance of `UnsafeCell` which will wrap the specified
        /// value.
        pub const fn new(data: T) -> Self {
            Self {
                data: cell::UnsafeCell::new(data),
            }
        }

        /// Get an immutable pointer to the wrapped value.
        ///
        /// # Panics
        ///
        /// When running under `loom`, this function will panic if the access is
        /// not valid under the Rust memory model.
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.data.get() as *const _)
        }

        /// Get a mutable pointer to the wrapped value.
        ///
        /// # Panics
        ///
        /// When running under `loom`, this function will panic if the access is
        /// not valid under the Rust memory model.
        pub fn with_mut<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.data.get())
        }

        /// Get an immutable pointer to the wrapped value.
        pub(crate) fn with_unchecked<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.data.get())
        }

        /// Get a mutable pointer to the wrapped value.
        pub(crate) fn with_mut_unchecked<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.data.get())
        }

        /// Get an immutable pointer to the wrapped value.
        ///
        /// This function returns a [`ConstPtr`] guard, which is analogous to a
        /// `*const T`, but tracked by Loom when the `cfg(loom)` cfg flag is
        /// enabled. As long as the returned `ConstPtr` exists, Loom will
        /// consider the cell to be accessed immutably.
        ///
        /// This means that any mutable accesses (e.g. calls to [`with_mut`] or
        /// [`get_mut`]) while the returned guard is live will result in a panic.
        ///
        /// # Panics
        ///
        /// This function will panic if the access is not valid under the Rust memory
        /// model, if `cfg(loom)` is enabled.
        ///
        /// [`with_mut`]: UnsafeCell::with_mut
        /// [`get_mut`]: UnsafeCell::get_mut
        #[inline(always)]
        #[must_use]
        pub fn get(&self) -> ConstPtr<T> {
            ConstPtr(self.data.get())
        }

        /// Get a mutable pointer to the wrapped value.
        ///
        /// This function returns a [`MutPtr`] guard, which is analogous to a
        /// `*mut T`, but tracked by Loom when the `cfg(loom)` cfg flag is
        /// enabled. As long as the returned `MutPtr`  exists, Loom will
        /// consider the cell to be accessed mutably.
        ///
        /// This means that any concurrent mutable or immutable accesses (e.g. calls
        /// to [`with`], [`with_mut`], [`get`], or [`get_mut`]) while the returned
        /// guard is live will result in a panic.
        ///
        /// # Panics
        ///
        /// This function will panic if the access is not valid under the Rust memory
        /// model, if `cfg(loom)` is enabled.
        ///
        /// [`with`]: UnsafeCell::with
        /// [`with_mut`]: UnsafeCell::with_mut
        /// [`get`]: UnsafeCell::get
        /// [`get_mut`]: UnsafeCell::get_mut
        #[inline(always)]
        #[must_use]
        pub fn get_mut(&self) -> MutPtr<T> {
            MutPtr(self.data.get())
        }
    }

    // === impl MutPtr ===

    impl<T: ?Sized> MutPtr<T> {
        /// Dereference the raw pointer.
        ///
        /// # Safety
        ///
        /// This is equivalent to dereferencing a `*mut T` pointer, so all the same
        /// safety considerations apply here.
        ///
        /// Because the `MutPtr` type can only be created by calling
        /// [`UnsafeCell::get_mut`] on a valid `UnsafeCell`, we know the pointer
        /// will never be null.
        ///
        /// Loom tracks whether the value contained in the [`UnsafeCell`] from which
        /// this pointer originated is being concurrently accessed, and will panic
        /// if a data race could occur. However, `loom` does _not_ track liveness
        /// --- the [`UnsafeCell`] this pointer points to may have been dropped.
        /// Therefore, the caller is responsible for ensuring this pointer is not
        /// dangling.
        ///
        // Clippy knows that it's Bad and Wrong to construct a mutable reference
        // from an immutable one...but this function is intended to simulate a raw
        // pointer, so we have to do that here.
        #[allow(clippy::mut_from_ref)]
        #[inline(always)]
        #[must_use]
        pub unsafe fn deref(&self) -> &mut T {
            &mut *self.0
        }

        /// Perform an operation with the actual value of the raw pointer.
        ///
        /// This may be used to call functions like [`ptr::write`], [`ptr::read]`,
        /// and [`ptr::eq`], which are not exposed by the `MutPtr` type, cast the
        /// pointer to an integer, et cetera.
        ///
        /// # Correct Usage
        ///
        /// Note that the raw pointer passed into the closure *must not* be moved
        /// out of the closure, as doing so will allow it to "escape" Loom's ability
        /// to track accesses.
        ///
        /// Loom considers the [`UnsafeCell`] from which this pointer originated to
        /// be "accessed mutably" as long as the [`MutPtr`] guard exists. When the
        /// guard is dropped, Loom considers the mutable access to have ended. This
        /// means that if the `*mut T` passed to a `with` closure is moved _out_ of
        /// that closure, it may outlive the guard, and thus exist past the end of
        /// the mutable access (as understood by Loom).
        ///
        /// For example, code like this is incorrect:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        /// let cell = UnsafeCell::new(1);
        ///
        /// let ptr = {
        ///     let tracked_ptr = cell.get_mut(); // tracked mutable access begins here
        ///
        ///      // move the real pointer out of the simulated pointer
        ///     tracked_ptr.with(|real_ptr| real_ptr)
        ///
        /// }; // tracked mutable access *ends here* (when the tracked pointer is dropped)
        ///
        /// // now, we can mutate the value *without* loom knowing it is being mutably
        /// // accessed! this is BAD NEWS --- if the cell was being accessed concurrently,
        /// // loom would have failed to detect the error!
        /// unsafe { (*ptr) = 2 }
        /// ```
        ///
        /// More subtly, if a *new* pointer is constructed from the original
        /// pointer, that pointer is not tracked by Loom, either. This might occur
        /// when constructing a pointer to a struct field or array index. For
        /// example, this is incorrect:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        ///
        /// struct MyStruct {
        ///     foo: usize,
        ///     bar: usize,
        /// }
        ///
        /// let my_struct = UnsafeCell::new(MyStruct { foo: 1, bar: 1});
        ///
        /// fn get_bar(cell: &UnsafeCell<MyStruct>) -> *mut usize {
        ///     let tracked_ptr = cell.get_mut(); // tracked mutable access begins here
        ///
        ///     tracked_ptr.with(|ptr| unsafe {
        ///         &mut (*ptr).bar as *mut usize
        ///     })
        /// } // tracked mutable access ends here, when `tracked_ptr` is dropped
        ///
        ///
        /// // now, a pointer to `mystruct.bar` exists that Loom is not aware of!
        /// // if we were to mutate `mystruct.bar` through this pointer while another
        /// // thread was accessing `mystruct` concurrently, Loom would fail to detect
        /// /// this.
        /// let ptr_to_bar = get_bar(&my_struct);
        /// ```
        ///
        /// Similarly, constructing pointers via pointer math (such as [`offset`])
        /// may also escape Loom's ability to track accesses.
        ///
        /// Finally, the raw pointer passed to the `with` closure may only be passed
        /// into function calls that don't take ownership of that pointer past the
        /// end of the function call. Therefore, code like this is okay:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        ///
        /// let cell = UnsafeCell::new(1);
        ///
        /// let ptr = cell.get_mut();
        /// let value_in_cell = ptr.with(|ptr| unsafe {
        ///     // This is fine, because `ptr::write` does not retain ownership of
        ///     // the pointer after when the function call returns.
        ///     core::ptr::write(ptr, 2)
        /// });
        /// ```
        ///
        /// But code like *this* is not okay:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        /// use core::sync::atomic::{AtomicPtr, Ordering};
        ///
        /// static SOME_IMPORTANT_POINTER: AtomicPtr<usize> = AtomicPtr::new(core::ptr::null_mut());
        ///
        /// fn mess_with_important_pointer(cell: &UnsafeCell<usize>) {
        ///     cell.get_mut() // mutable access begins here
        ///        .with(|ptr| {
        ///             SOME_IMPORTANT_POINTER.store(ptr, Ordering::SeqCst);
        ///         })
        /// } // mutable access ends here
        ///
        /// // loom doesn't know that the cell can still be accessed via the `AtomicPtr`!
        /// ```
        ///
        /// [`ptr::write`]: core::ptr::write
        /// [`ptr::read`]: core::ptr::read
        /// [`ptr::eq`]: core::ptr::eq
        /// [`offset`]: https://doc.rust-lang.org/stable/core/primitive.pointer.html#method.offset
        #[inline(always)]
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*mut T) -> R,
        {
            f(self.0)
        }
    }

    // === impl ConstPtr ===

    impl<T: ?Sized> ConstPtr<T> {
        /// Dereference the raw pointer.
        ///
        /// # Safety
        ///
        /// This is equivalent to dereferencing a `*const T` pointer, so all the
        /// same safety considerations apply here.
        ///
        ///
        /// Because the `ConstPtr` type can only be created by calling
        /// [`UnsafeCell::get_mut`] on a valid `UnsafeCell`, we know the pointer
        /// will never be null.
        ///
        /// Loom tracks whether the value contained in the [`UnsafeCell`] from which
        /// this pointer originated is being concurrently accessed, and will panic
        /// if a data race could occur. However, `loom` does _not_ track liveness
        /// --- the [`UnsafeCell`] this pointer points to may have been dropped.
        /// Therefore, the caller is responsible for ensuring this pointer is not
        /// dangling.
        ///
        #[inline(always)]
        #[must_use]
        pub unsafe fn deref(&self) -> &T {
            &*self.0
        }

        /// Perform an operation with the actual value of the raw pointer.
        ///
        /// This may be used to call functions like [`ptr::read]` and [`ptr::eq`],
        /// which are not exposed by the `ConstPtr` type, cast the pointer to an
        /// integer, et cetera.
        ///
        /// # Correct Usage
        ///
        /// Note that the raw pointer passed into the closure *must not* be moved
        /// out of the closure, as doing so will allow it to "escape" Loom's ability
        /// to track accesses.
        ///
        /// Loom considers the [`UnsafeCell`] from which this pointer originated to
        /// be "accessed immutably" as long as the [`ConstPtr`] guard exists. When the
        /// guard is dropped, Loom considers the immutable access to have ended. This
        /// means that if the `*const T` passed to a `with` closure is moved _out_ of
        /// that closure, it may outlive the guard, and thus exist past the end of
        /// the access (as understood by Loom).
        ///
        /// For example, code like this is incorrect:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        /// let cell = UnsafeCell::new(1);
        ///
        /// let ptr = {
        ///     let tracked_ptr = cell.get(); // tracked immutable access begins here
        ///
        ///      // move the real pointer out of the simulated pointer
        ///     tracked_ptr.with(|real_ptr| real_ptr)
        ///
        /// }; // tracked immutable access *ends here* (when the tracked pointer is dropped)
        ///
        /// // now, another thread can mutate the value *without* loom knowing it is being
        /// // accessed concurrently by this thread! this is BAD NEWS ---  loom would have
        /// // failed to detect a potential data race!
        /// unsafe { println!("{}", (*ptr)) }
        /// ```
        ///
        /// More subtly, if a *new* pointer is constructed from the original
        /// pointer, that pointer is not tracked by Loom, either. This might occur
        /// when constructing a pointer to a struct field or array index. For
        /// example, this is incorrect:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        ///
        /// struct MyStruct {
        ///     foo: usize,
        ///     bar: usize,
        /// }
        ///
        /// let my_struct = UnsafeCell::new(MyStruct { foo: 1, bar: 1});
        ///
        /// fn get_bar(cell: &UnsafeCell<MyStruct>) -> *const usize {
        ///     let tracked_ptr = cell.get(); // tracked immutable access begins here
        ///
        ///     tracked_ptr.with(|ptr| unsafe {
        ///         &(*ptr).bar as *const usize
        ///     })
        /// } // tracked access ends here, when `tracked_ptr` is dropped
        ///
        ///
        /// // now, a pointer to `mystruct.bar` exists that Loom is not aware of!
        /// // if another thread were to mutate `mystruct.bar` while we are holding this
        /// // pointer, Loom would fail to detect the data race!
        /// let ptr_to_bar = get_bar(&my_struct);
        /// ```
        ///
        /// Similarly, constructing pointers via pointer math (such as [`offset`])
        /// may also escape Loom's ability to track accesses.
        ///
        /// Furthermore, the raw pointer passed to the `with` closure may only be passed
        /// into function calls that don't take ownership of that pointer past the
        /// end of the function call. Therefore, code like this is okay:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        ///
        /// let cell = UnsafeCell::new(1);
        ///
        /// let ptr = cell.get();
        /// let value_in_cell = ptr.with(|ptr| unsafe {
        ///     // This is fine, because `ptr::read` does not retain ownership of
        ///     // the pointer after when the function call returns.
        ///     core::ptr::read(ptr)
        /// });
        /// ```
        ///
        /// But code like *this* is not okay:
        ///
        /// ```rust
        /// use mycelium_util::sync::cell::UnsafeCell;
        /// use core::ptr;
        ///
        /// struct ListNode<T> {
        ///    value: *const T,
        ///    next: *const ListNode<T>,
        /// }
        ///
        /// impl<T> ListNode<T> {
        ///     fn new(value: *const T) -> Box<Self> {
        ///         Box::new(ListNode {
        ///             value, // the pointer is moved into the `ListNode`, which will outlive this function!
        ///             next: ptr::null::<ListNode<T>>(),
        ///         })
        ///     }
        /// }
        ///
        /// let cell = UnsafeCell::new(1);
        ///
        /// let ptr = cell.get(); // immutable access begins here
        ///
        /// // the pointer passed into `ListNode::new` will outlive the function call
        /// let node = ptr.with(|ptr| ListNode::new(ptr));
        ///
        /// drop(ptr); // immutable access ends here
        ///
        /// // loom doesn't know that the cell can still be accessed via the `ListNode`!
        /// ```
        ///
        /// Finally, the `*const T` passed to `with` should *not* be cast to an
        /// `*mut T`. This will permit untracked mutable access, as Loom only tracks
        /// the existence of a `ConstPtr` as representing an immutable access.
        ///
        /// [`ptr::read`]: core::ptr::read
        /// [`ptr::eq`]: core::ptr::eq
        /// [`offset`]: https://doc.rust-lang.org/stable/core/primitive.pointer.html#method.offset
        #[inline(always)]
        pub fn with<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const T) -> R,
        {
            f(self.0)
        }
    }
}
