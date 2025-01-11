#![allow(dead_code)] // most of this isn't used yet...
use core::{fmt, mem::MaybeUninit};

/// A checked version of [`core::mem::MaybeUninit`].
///
/// This is similar to [`core::mem::MaybeUninit`] in release builds. In debug
/// mode builds, it additionally stores a flag tracking whether the value is
/// initialized, and asserts that the cell is initialized when it is accessed.
///
/// # Differences from `MaybeUninit`
///
/// This type is **not** capable of tracking initialization of
/// partially-initialized values, so it lacks `core::mem::MaybeUninit`'s array
/// and slice methods. Additionally, it does not implement a version of
/// [`MaybeUninit::zeroed`], because it does not know whether a zeroed `T` is
/// valid or not.
pub struct CheckedMaybeUninit<T> {
    value: MaybeUninit<T>,
    #[cfg(debug_assertions)]
    initialized: bool,
}

impl<T> CheckedMaybeUninit<T> {
    /// Creates a new `CheckedMaybeUninit<T>` initialized with the given value.
    /// It is safe to call [`assume_init`] on the return value of this function.
    ///
    /// Note that dropping a `CheckedMaybeUninit<T>` will never call `T`'s drop code.
    /// It is your responsibility to make sure `T` gets dropped if it got initialized.
    ///
    /// [`assume_init`]: Self::assume_init
    #[must_use = "use `forget` to avoid running Drop code"]
    #[inline(always)]
    pub const fn new(val: T) -> Self {
        Self {
            value: MaybeUninit::new(val),
            #[cfg(debug_assertions)]
            initialized: true,
        }
    }

    /// Creates a new `CheckedMaybeUninit<T>` in an uninitialized state.
    ///
    /// Note that dropping a `CheckedMaybeUninit<T>` will never call `T`'s drop code.
    /// It is your responsibility to make sure `T` gets dropped if it got initialized.
    ///
    /// See the [type-level documentation][CheckedMaybeUninit] for some examples.
    #[must_use]
    #[inline(always)]
    pub const fn uninit() -> Self {
        Self {
            value: MaybeUninit::uninit(),
            #[cfg(debug_assertions)]
            initialized: false,
        }
    }

    /// Sets the value of the `CheckedMaybeUninit<T>`.
    ///
    /// This overwrites any previous value without dropping it, so be careful
    /// not to use this twice unless you want to skip running the destructor.
    /// For your convenience, this also returns a mutable reference to the
    /// (now safely initialized) contents of `self`.
    ///
    /// As the content is stored inside a `CheckedMaybeUninit`, the destructor is not
    /// run for the inner data if the MaybeUninit leaves scope without a call to
    /// [`assume_init`], [`assume_init_drop`], or similar. Code that receives
    /// the mutable reference returned by this function needs to keep this in
    /// mind. The safety model of Rust regards leaks as safe, but they are
    /// usually still undesirable. This being said, the mutable reference
    /// behaves like any other mutable reference would, so assigning a new value
    /// to it will drop the old content.
    ///
    /// [`assume_init`]: Self::assume_init
    /// [`assume_init_drop`]: Self::assume_init_drop
    #[inline(always)]
    pub fn write(&mut self, val: T) -> &mut T {
        self.init().write(val)
    }

    /// Gets a pointer to the contained value. Reading from this pointer or turning it
    /// into a reference is undefined behavior unless the `CheckedMaybeUninit<T>` is initialized.
    /// Writing to memory that this pointer (non-transitively) points to is undefined behavior
    /// (except inside an `UnsafeCell<T>`).
    #[inline(always)]
    #[track_caller]
    pub fn as_ptr(&self) -> *const T {
        self.assert_init("as_ptr").as_ptr()
    }

    /// Gets a mutable pointer to the contained value. Reading from this pointer or turning it
    /// into a reference is undefined behavior unless the `CheckedMaybeUninit<T>` is initialized.
    #[inline(always)]
    #[track_caller]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.assert_init_mut("as_mut_ptr").as_mut_ptr()
    }

    /// Extracts the value from the `CheckedMaybeUninit<T>` container. This is a great way
    /// to ensure that the data will get dropped, because the resulting `T` is
    /// subject to the usual drop handling.
    ///
    /// # Safety
    ///
    /// It is up to the caller to guarantee that the `CheckedMaybeUninit<T>` really is in an initialized
    /// state. Calling this when the content is not yet fully initialized causes immediate undefined
    /// behavior. The [type-level documentation][inv] contains more information about
    /// this initialization invariant.
    ///
    /// [inv]: #initialization-invariant
    ///
    /// On top of that, remember that most types have additional invariants beyond merely
    /// being considered initialized at the type level. For example, a `1`-initialized [`Vec<T>`]
    /// is considered initialized (under the current implementation; this does not constitute
    /// a stable guarantee) because the only requirement the compiler knows about it
    /// is that the data pointer must be non-null. Creating such a `Vec<T>` does not cause
    /// *immediate* undefined behavior, but will cause undefined behavior with most
    /// safe operations (including dropping it).
    ///
    /// [`Vec<T>`]: ../../std/vec/struct.Vec.html
    ///
    /// # Examples
    ///
    /// Correct usage of this method:
    ///
    /// ```rust
    /// use std::mem::MaybeUninit;
    ///
    /// let mut x = MaybeUninit::<bool>::uninit();
    /// x.write(true);
    /// let x_init = unsafe { x.assume_init() };
    /// assert_eq!(x_init, true);
    /// ```
    ///
    /// *Incorrect* usage of this method:
    ///
    /// ```rust,no_run
    /// use std::mem::MaybeUninit;
    ///
    /// let x = MaybeUninit::<Vec<u32>>::uninit();
    /// let x_init = unsafe { x.assume_init() };
    /// // `x` had not been initialized yet, so this last line caused undefined behavior. ⚠️
    /// ```
    #[inline(always)]
    #[track_caller]
    pub unsafe fn assume_init(self) -> T {
        self.assert_init_val("assume_init").assume_init()
    }

    /// Reads the value from the `CheckedMaybeUninit<T>` container. The resulting `T` is subject
    /// to the usual drop handling.
    ///
    /// Whenever possible, it is preferable to use [`assume_init`] instead, which
    /// prevents duplicating the content of the `CheckedMaybeUninit<T>`.
    ///
    /// # Safety
    ///
    /// It is up to the caller to guarantee that the `CheckedMaybeUninit<T>` really is in an initialized
    /// state. Calling this when the content is not yet fully initialized causes undefined
    /// behavior. The [type-level documentation][inv] contains more information about
    /// this initialization invariant.
    ///
    /// Moreover, similar to the [`ptr::read`] function, this function creates a
    /// bitwise copy of the contents, regardless whether the contained type
    /// implements the [`Copy`] trait or not. When using multiple copies of the
    /// data (by calling `assume_init_read` multiple times, or first calling
    /// `assume_init_read` and then [`assume_init`]), it is your responsibility
    /// to ensure that that data may indeed be duplicated.
    ///
    /// [inv]: #initialization-invariant
    /// [`assume_init`]: MaybeUninit::assume_init
    /// [`ptr::read`]: core::ptr::read
    #[inline(always)]
    #[track_caller]
    pub unsafe fn assume_init_read(&self) -> T {
        self.assert_init("assume_init_read").assume_init_read()
    }

    /// Drops the contained value in place.
    ///
    /// If you have ownership of the `CheckedMaybeUninit`, you can also use
    /// [`assume_init`] as an alternative.
    ///
    /// # Safety
    ///
    /// It is up to the caller to guarantee that the `CheckedMaybeUninit<T>` really is
    /// in an initialized state. Calling this when the content is not yet fully
    /// initialized causes undefined behavior.
    ///
    /// On top of that, all additional invariants of the type `T` must be
    /// satisfied, as the `Drop` implementation of `T` (or its members) may
    /// rely on this. For example, setting a [`Vec<T>`] to an invalid but
    /// non-null address makes it initialized (under the current implementation;
    /// this does not constitute a stable guarantee), because the only
    /// requirement the compiler knows about it is that the data pointer must be
    /// non-null. Dropping such a `Vec<T>` however will cause undefined
    /// behaviour.
    ///
    /// [`assume_init`]: MaybeUninit::assume_init
    #[inline(always)]
    #[track_caller]
    pub unsafe fn assume_init_drop(&mut self) {
        self.assert_init_mut("assume_init_drop").assume_init_drop()
    }

    /// Gets a shared reference to the contained value.
    ///
    /// This can be useful when we want to access a `CheckedMaybeUninit` that has been
    /// initialized but don't have ownership of the `CheckedMaybeUninit` (preventing the use
    /// of `.assume_init()`).
    ///
    /// # Safety
    ///
    /// Calling this when the content is not yet fully initialized causes undefined
    /// behavior: it is up to the caller to guarantee that the `CheckedMaybeUninit<T>` really
    /// is in an initialized state.
    ///
    /// # Examples
    ///
    /// ### Correct usage of this method:
    ///
    /// ```rust
    /// use std::mem::MaybeUninit;
    ///
    /// let mut x = MaybeUninit::<Vec<u32>>::uninit();
    /// // Initialize `x`:
    /// x.write(vec![1, 2, 3]);
    /// // Now that our `CheckedMaybeUninit<_>` is known to be initialized, it is okay to
    /// // create a shared reference to it:
    /// let x: &Vec<u32> = unsafe {
    ///     // SAFETY: `x` has been initialized.
    ///     x.assume_init_ref()
    /// };
    /// assert_eq!(x, &vec![1, 2, 3]);
    /// ```
    ///
    /// ### *Incorrect* usages of this method:
    ///
    /// ```rust,no_run
    /// use std::mem::MaybeUninit;
    ///
    /// let x = MaybeUninit::<Vec<u32>>::uninit();
    /// let x_vec: &Vec<u32> = unsafe { x.assume_init_ref() };
    /// // We have created a reference to an uninitialized vector! This is undefined behavior. ⚠️
    /// ```
    ///
    /// ```rust,no_run
    /// use std::{cell::Cell, mem::MaybeUninit};
    ///
    /// let b = MaybeUninit::<Cell<bool>>::uninit();
    /// // Initialize the `CheckedMaybeUninit` using `Cell::set`:
    /// unsafe {
    ///     b.assume_init_ref().set(true);
    ///    // ^^^^^^^^^^^^^^^
    ///    // Reference to an uninitialized `Cell<bool>`: UB!
    /// }
    /// ```
    #[track_caller]
    #[inline(always)]
    pub unsafe fn assume_init_ref(&self) -> &T {
        self.assert_init("assume_init_ref").assume_init_ref()
    }

    /// Gets a mutable (unique) reference to the contained value.
    ///
    /// This can be useful when we want to access a `CheckedMaybeUninit` that has been
    /// initialized but don't have ownership of the `CheckedMaybeUninit` (preventing the use
    /// of `.assume_init()`).
    ///
    /// # Safety
    ///
    /// Calling this when the content is not yet fully initialized causes undefined
    /// behavior: it is up to the caller to guarantee that the `CheckedMaybeUninit<T>` really
    /// is in an initialized state. For instance, `.assume_init_mut()` cannot be used to
    /// initialize a `CheckedMaybeUninit`.
    ///
    /// # Examples
    ///
    /// ### Correct usage of this method:
    ///
    /// ```rust
    /// # #![allow(unexpected_cfgs)]
    /// use std::mem::MaybeUninit;
    ///
    /// # unsafe extern "C" fn initialize_buffer(buf: *mut [u8; 1024]) { *buf = [0; 1024] }
    /// # #[cfg(FALSE)]
    /// extern "C" {
    ///     /// Initializes *all* the bytes of the input buffer.
    ///     fn initialize_buffer(buf: *mut [u8; 1024]);
    /// }
    ///
    /// let mut buf = MaybeUninit::<[u8; 1024]>::uninit();
    ///
    /// // Initialize `buf`:
    /// unsafe { initialize_buffer(buf.as_mut_ptr()); }
    /// // Now we know that `buf` has been initialized, so we could `.assume_init()` it.
    /// // However, using `.assume_init()` may trigger a `memcpy` of the 1024 bytes.
    /// // To assert our buffer has been initialized without copying it, we upgrade
    /// // the `&mut MaybeUninit<[u8; 1024]>` to a `&mut [u8; 1024]`:
    /// let buf: &mut [u8; 1024] = unsafe {
    ///     // SAFETY: `buf` has been initialized.
    ///     buf.assume_init_mut()
    /// };
    ///
    /// // Now we can use `buf` as a normal slice:
    /// buf.sort_unstable();
    /// debug_assert!(
    ///     buf.windows(2).all(|pair| pair[0] <= pair[1]),
    ///     "buffer is sorted",
    /// );
    /// ```
    ///
    /// ### *Incorrect* usages of this method:
    ///
    /// You cannot use `.assume_init_mut()` to initialize a value:
    ///
    /// ```rust,no_run
    /// use std::mem::MaybeUninit;
    ///
    /// let mut b = MaybeUninit::<bool>::uninit();
    /// unsafe {
    ///     *b.assume_init_mut() = true;
    ///     // We have created a (mutable) reference to an uninitialized `bool`!
    ///     // This is undefined behavior. ⚠️
    /// }
    /// ```
    ///
    /// For instance, you cannot [`Read`] into an uninitialized buffer:
    ///
    /// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
    ///
    /// ```rust,no_run
    /// use std::{io, mem::MaybeUninit};
    ///
    /// fn read_chunk (reader: &'_ mut dyn io::Read) -> io::Result<[u8; 64]>
    /// {
    ///     let mut buffer = MaybeUninit::<[u8; 64]>::uninit();
    ///     reader.read_exact(unsafe { buffer.assume_init_mut() })?;
    ///                             // ^^^^^^^^^^^^^^^^^^^^^^^^
    ///                             // (mutable) reference to uninitialized memory!
    ///                             // This is undefined behavior.
    ///     Ok(unsafe { buffer.assume_init() })
    /// }
    /// ```
    ///
    /// Nor can you use direct field access to do field-by-field gradual initialization:
    ///
    /// ```rust,no_run
    /// use std::{mem::MaybeUninit, ptr};
    ///
    /// struct Foo {
    ///     a: u32,
    ///     b: u8,
    /// }
    ///
    /// let foo: Foo = unsafe {
    ///     let mut foo = MaybeUninit::<Foo>::uninit();
    ///     ptr::write(&mut foo.assume_init_mut().a as *mut u32, 1337);
    ///                  // ^^^^^^^^^^^^^^^^^^^^^
    ///                  // (mutable) reference to uninitialized memory!
    ///                  // This is undefined behavior.
    ///     ptr::write(&mut foo.assume_init_mut().b as *mut u8, 42);
    ///                  // ^^^^^^^^^^^^^^^^^^^^^
    ///                  // (mutable) reference to uninitialized memory!
    ///                  // This is undefined behavior.
    ///     foo.assume_init()
    /// };
    /// ```
    #[inline(always)]
    #[track_caller]
    pub unsafe fn assume_init_mut(&mut self) -> &mut T {
        self.assert_init_mut("assume_init_mut").assume_init_mut()
    }

    #[inline(always)]
    fn init(&mut self) -> &mut MaybeUninit<T> {
        #[cfg(debug_assertions)]
        {
            self.initialized = true;
        }
        &mut self.value
    }

    #[inline(always)]
    #[track_caller]
    fn assert_init(&self, _method: &'static str) -> &MaybeUninit<T> {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.initialized,
            "`MaybeUninit::{_method}` called on a `MaybeUninit` cell that was not initialized! this is a bug!",
        );
        &self.value
    }

    #[inline(always)]
    #[track_caller]
    fn assert_init_mut(&mut self, _method: &'static str) -> &mut MaybeUninit<T> {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.initialized,
            "`MaybeUninit::{_method}` called on a `MaybeUninit` cell that was not initialized! this is a bug!",
        );
        &mut self.value
    }

    #[inline(always)]
    #[track_caller]
    fn assert_init_val(self, _method: &'static str) -> MaybeUninit<T> {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.initialized,
            "`MaybeUninit::{_method}` called on a `MaybeUninit` cell that was not initialized! this is a bug!",
        );
        self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for CheckedMaybeUninit<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("CheckedMaybeUninit");
        #[cfg(debug_assertions)]
        if self.initialized {
            s.field("value", unsafe { &self.assume_init_ref() });
        } else {
            s.field("value", &format_args!("<uninitialized>"));
        }

        #[cfg(not(test))]
        {
            s.field("value", &format_args!("<maybe uninitialized>"));
        }

        s.finish()
    }
}
