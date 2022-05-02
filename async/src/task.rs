use crate::loom::UnsafeCell;
use alloc::boxed::Box;
pub use core::task::{Context, Poll, Waker};
use core::{future::Future, pin::Pin, ptr::NonNull};
use cordyceps::list;

mod task_list;

#[repr(C)]
pub struct Task {
    header: Header,
    future: dyn Future<Output = ()>,
}

pub(crate) struct Header {
    task_list: UnsafeCell<list::Links<Self>>,
}

// === impl Task ===

/// # Safety
///
/// A task must be pinned to be spawned.
unsafe impl list::Linked for Task {
    type Handle = Pin<*mut Header>;
    type Node = Header;

    fn as_ptr(task: &Self::Handle) -> NonNull<Self::Node> {
        NonNull::from(&task.header)
    }

    /// Convert a raw pointer to a `Handle`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn from_ptr(ptr: NonNull<Self::Node>) -> Self::Handle {
        Pin::new_unchecked(Box::from_raw(ptr.as_ptr() as *mut _))
    }

    /// Return the links of the node pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function is safe to call when:
    /// - It is valid to construct a `Handle` from a`raw pointer
    /// - The pointer points to a valid instance of `Self` (e.g. it does not
    ///   dangle).
    unsafe fn links(ptr: NonNull<Self::Node>) -> NonNull<list::Links<Self::Node>> {
        ptr.as_ref().task_list.with_mut(|links| NonNull::new_unchecked(links)
    }
}

// === impl Header ===
unsafe impl Send for Header {}
unsafe impl Sync for Header {}
