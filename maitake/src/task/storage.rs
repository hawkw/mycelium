use super::Task;
use core::future::Future;
use core::ptr::NonNull;

/// A trait representing a heap allocation that can own a [`Task`].
///
/// This is used to contain tasks at runtime, and abstract over the
/// type erasure and type recovery steps, e.g. converting a heap
/// allocation type into a [`NonNull`] pointer, and recovering it
/// back into a heap allocation from a [`NonNull`] pointer
///
/// This trait is exposed publicly to allow for end users to implement
/// for their own heap allocation types, if tasks must be stored in
/// an allocation type other than [`Box`].
///
/// This trait is ONLY appropriate for heap allocation types that represent
/// exclusive ownership of the contained data, as it may be mutated while
/// in pointer form. For example, the `Box` type would match this guarantee,
/// but the `Arc` type would not, as it allows for shared access, and NOT
/// exclusive mutable access.
///
/// **Note**: The type that implements this trait is typically NOT the heap
/// allocation type itself, but a "Marker Type" that represents the
/// intended storage medium. See the [`BoxStorage`] type (available with the
/// "alloc" feature active) for an implementation example
///
/// [`Task`]: crate::task::Task
/// [`Box`]: alloc::boxed::Box
/// [`BoxStorage`]: crate::task::BoxStorage
pub trait Storage<S, F: Future>: Sized {
    /// The type of a stored Task.
    ///
    /// As the type that implements the Storage trait is a Marker Type,
    /// This associated type is the actual heap type that will be used
    /// to contain the Task.
    type StoredTask;

    /// Convert an owned, heap-allocated [`Task`] type to a raw pointer
    ///
    /// This method should produce a [`NonNull`] pointer, while not actually
    /// dropping the contained task.
    fn into_raw(task: Self::StoredTask) -> NonNull<Task<S, F, Self>>;

    /// Convert a raw task pointer into an owned, heap`allocated [`Task`] type
    ///
    /// This method should produce a heap-allocated type, which can be
    /// dropped to perform the correct destructor actions
    fn from_raw(ptr: NonNull<Task<S, F, Self>>) -> Self::StoredTask;
}

feature! {
    #![feature = "alloc"]
    use alloc::boxed::Box;

    /// A type representing [`Box`] storage of a task
    ///
    /// [`Box`]: alloc::boxed::Box
    pub struct BoxStorage;

    impl<S, F: Future> Storage<S, F> for BoxStorage {
        type StoredTask = Box<Task<S, F, BoxStorage>>;

        fn into_raw(task: Box<Task<S, F, BoxStorage>>) -> NonNull<Task<S, F, BoxStorage>> {
            unsafe {
                crate::util::non_null(Box::into_raw(task))
            }
        }

        fn from_raw(ptr: NonNull<Task<S, F, BoxStorage>>) -> Box<Task<S, F, BoxStorage>> {
            unsafe { Box::from_raw(ptr.as_ptr()) }
        }
    }

}
