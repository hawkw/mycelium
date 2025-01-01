//! Deferred closure execution (a.k.a. scope guards).
//!
//! See the [`Deferred`] type and the [`defer`] function for more information.

/// Defers execution of a closure until a scope is exited.
///
/// As seen in "The Go Programming Language".
#[must_use = "dropping a `Deferred` guard immediately executes \
    the deferred function"]
pub struct Deferred<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Deferred<F> {
    /// Defer execution of `f` until this scope is exited.
    #[inline]
    pub const fn new(f: F) -> Self {
        Self(Some(f))
    }

    /// Cancel the deferred execution.of the closure passed to `defer`.
    ///
    /// Calling this method will prevent the closure from being executed when
    /// this `Deferred` guard is dropped.
    #[inline]
    pub fn cancel(&mut self) {
        self.0 = None;
    }
}

impl<F: FnOnce()> Drop for Deferred<F> {
    #[inline]
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f()
        }
    }
}

impl<F: FnOnce()> core::fmt::Debug for Deferred<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Deferred")
            .field(&format_args!("{}", core::any::type_name::<F>()))
            .finish()
    }
}

/// Defer execution of `f` until this scope is exited.
///
/// This is equivalent to calling `Deferred::new(f)`. See [`Deferred`] for more
/// details.
#[inline(always)]
pub fn defer<F: FnOnce()>(f: F) -> Deferred<F> {
    Deferred::new(f)
}
