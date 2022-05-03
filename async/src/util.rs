use core::ptr::NonNull;

/// Helper to construct a `NonNull<T>` from a raw pointer to `T`, with null
/// checks elided in release mode.
#[cfg(debug_assertions)]
#[track_caller]
#[inline(always)]
pub(crate) unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new(ptr).expect(
        "/!\\ constructed a `NonNull` from a null pointer! /!\\ \n\
        in release mode, this would have called `NonNull::new_unchecked`, \
        violating the `NonNull` invariant! this is a bug in `cordyceps!`.",
    )
}

/// Helper to construct a `NonNull<T>` from a raw pointer to `T`, with null
/// checks elided in release mode.
///
/// This is the release mode version.
#[cfg(not(debug_assertions))]
#[inline(always)]
unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
    NonNull::new_unchecked(ptr)
}
