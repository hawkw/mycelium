//! Abstractions for core-local storage.

/// A core-local storage cell.
///
/// This trait represents an architecture-specific mechanism for storing local
/// data for each CPU core.
pub trait CoreLocal<T: 'static> {
    /// Returns a new instance of `Self`, using the provided `init` function to
    /// generate the initial value for each CPU core.
    fn new(init: fn() -> T) -> Self;

    /// Accesses the value for the current CPU core.
    ///
    /// This method invokes the provided closure `f` with a reference to the
    /// `T`-typed local data for the current CPU core.
    fn with<F, U>(&self, f: F) -> U
    where
        F: FnOnce(&T) -> U;
}
