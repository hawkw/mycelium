//! An [`Error`] trait based on [`std::error::Error`].
//!
//! This is a modified version of [`std::error::Error`] that doesn't require the
//! Rust standard library.
use core::{any::TypeId, fmt};

/// `Error` is a trait representing the basic expectations for error values,
/// i.e., values of type `E` in `Result<T, E>`.
///
/// This is essentially a re-implementation of the Rust standard library's
/// `std::error::Error` trait. Because we cannot use `std` in the kernel, we
/// must reimplement that trait in `mycelium_util`.
///
/// Errors must describe themselves through the `Display` and `Debug` traits,
/// and may provide cause chain information:
///
/// The [`source`] method is generally used when errors cross "abstraction
/// boundaries". If one module must report an error that is caused by an error
/// from a lower-level module, it can allow access to that error via the
/// [`source`] method. This makes it possible for the high-level module to
/// provide its own errors while also revealing some of the implementation for
/// debugging via [`source`] chains.
///
/// [`source`]: trait.Error.html#method.source
pub trait Error: fmt::Debug + fmt::Display {
    /// The lower-level source of this error, if any.
    ///
    /// This is equivalent to `std::error::Error`'s `source` method.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    /// Gets the `TypeId` of `self`.
    fn type_id(&self, _: private::Internal) -> TypeId
    where
        Self: 'static,
    {
        TypeId::of::<Self>()
    }
}

impl dyn Error + 'static + Send {
    /// Forwards to the method defined on the type `dyn Error`.
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        <dyn Error + 'static>::is::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        <dyn Error + 'static>::downcast_ref::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[inline]
    pub fn downcast_mut<T: Error + 'static>(&mut self) -> Option<&mut T> {
        <dyn Error + 'static>::downcast_mut::<T>(self)
    }
}

impl dyn Error + 'static + Send + Sync {
    /// Forwards to the method defined on the type `dyn Error`.
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        <dyn Error + 'static>::is::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        <dyn Error + 'static>::downcast_ref::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[inline]
    pub fn downcast_mut<T: Error + 'static>(&mut self) -> Option<&mut T> {
        <dyn Error + 'static>::downcast_mut::<T>(self)
    }
}

impl dyn Error {
    /// Returns an iterator starting with the current error and continuing with
    /// recursively calling [`source`].
    ///
    /// [`source`]: trait.Error.html#method.source
    #[inline]
    pub fn iter_chain(&self) -> ErrorIter<'_> {
        ErrorIter {
            current: Some(self),
        }
    }

    /// Returns an iterator starting with the [`source`] of this error
    /// and continuing with recursively calling [`source`].
    ///
    /// [`source`]: trait.Error.html#method.source
    #[inline]
    pub fn iter_sources(&self) -> ErrorIter<'_> {
        ErrorIter {
            current: self.source(),
        }
    }
}

impl dyn Error + 'static {
    /// Returns `true` if the boxed type is the same as `T`
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        // Get `TypeId` of the type this function is instantiated with.
        let t = TypeId::of::<T>();

        // Get `TypeId` of the type in the trait object.
        let obj = self.type_id(private::Internal);

        // Compare both `TypeId`s on equality.
        t == obj
    }

    /// Returns some reference to the boxed value if it is of type `T`, or
    /// `None` if it isn't.
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(&*(self as *const dyn Error as *const T)) }
        } else {
            None
        }
    }

    /// Returns some mutable reference to the boxed value if it is of type `T`, or
    /// `None` if it isn't.
    #[inline]
    pub fn downcast_mut<T: Error + 'static>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(&mut *(self as *mut dyn Error as *mut T)) }
        } else {
            None
        }
    }
}

/// An iterator over [`Error`]
///
/// [`Error`]: trait.Error.html
#[derive(Copy, Clone, Debug)]
pub struct ErrorIter<'a> {
    current: Option<&'a (dyn Error + 'static)>,
}

impl<'a> Iterator for ErrorIter<'a> {
    type Item = &'a (dyn Error + 'static);

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current;
        self.current = self.current.and_then(Error::source);
        current
    }
}

impl Error for &'static str {}

mod private {
    // This is a hack to prevent `type_id` from being overridden by `Error`
    // implementations, since that can enable unsound downcasting.
    #[derive(Debug)]
    pub struct Internal;
}
