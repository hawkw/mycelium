//! A multi-producer, single-consumer (MPSC) queue, implemented using a
//! lock-free [intrusive] singly-linked list.
//!
//! See the documentation for the [`MpscQueue`] type for details.
//!
//! Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
//!
//! [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
//! [intrusive]: crate#intrusive-data-structures
use crate::{
    loom::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, AtomicPtr, Ordering::*},
    },
    util::{Backoff, CachePadded},
    Linked,
};
use core::{
    fmt,
    marker::PhantomPinned,
    ptr::{self, NonNull},
};

/// A multi-producer, single-consumer (MPSC) queue, implemented using a
/// lock-free [intrusive] singly-linked list.
///
/// Based on [Dmitry Vyukov's intrusive MPSC][vyukov].
///
/// In order to be part of a `MpscQueue`, a type `T` must implement [`Linked`] for
/// [`mpsc_queue::Links<T>`].
///
/// [`mpsc_queue::Links<T>`]: crate::mpsc_queue::Links
///
/// # Examples
///
/// ```
/// use cordyceps::{
///     Linked,
///     mpsc_queue::{self, MpscQueue},
/// };
///
/// // This example uses the Rust standard library for convenience, but
/// // the MPSC queue itself does not require std.
/// use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
///
/// /// A simple queue entry that stores an `i32`.
/// #[derive(Debug, Default)]
/// struct Entry {
///    links: mpsc_queue::Links<Entry>,
///    val: i32,
/// }
///
/// // Implement the `Linked` trait for our entry type so that it can be used
/// // as a queue entry.
/// unsafe impl Linked<mpsc_queue::Links<Entry>> for Entry {
///     // In this example, our entries will be "owned" by a `Box`, but any
///     // heap-allocated type that owns an element may be used.
///     //
///     // An element *must not* move while part of an intrusive data
///     // structure. In many cases, `Pin` may be used to enforce this.
///     type Handle = Pin<Box<Self>>;
///
///     /// Convert an owned `Handle` into a raw pointer
///     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
///        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
///     }
///
///     /// Convert a raw pointer back into an owned `Handle`.
///     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
///         // Safety: if this function is only called by the linked list
///         // implementation (and it is not intended for external use), we can
///         // expect that the `NonNull` was constructed from a reference which
///         // was pinned.
///         //
///         // If other callers besides `MpscQueue`'s internals were to call this on
///         // some random `NonNull<Entry>`, this would not be the case, and
///         // this could be constructing an erroneous `Pin` from a referent
///         // that may not be pinned!
///         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
///     }
///
///     /// Access an element's `Links`.
///     unsafe fn links(target: NonNull<Entry>) -> NonNull<mpsc_queue::Links<Entry>> {
///         // Using `ptr::addr_of_mut!` permits us to avoid creating a temporary
///         // reference without using layout-dependent casts.
///         let links = ptr::addr_of_mut!((*target.as_ptr()).links);
///
///         // `NonNull::new_unchecked` is safe to use here, because the pointer that
///         // we offset was not null, implying that the pointer produced by offsetting
///         // it will also not be null.
///         NonNull::new_unchecked(links)
///     }
/// }
///
/// impl Entry {
///     fn new(val: i32) -> Self {
///         Self {
///             val,
///             ..Self::default()
///         }
///     }
/// }
///
/// // Once we have a `Linked` implementation for our element type, we can construct
/// // a queue.
///
/// // Because `Pin<Box<...>>` doesn't have a `Default` impl, we have to manually
/// // construct the stub node.
/// let stub = Box::pin(Entry::default());
/// let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));
///
/// // Spawn some producer threads.
/// thread::spawn({
///     let q = q.clone();
///     move || {
///         // Enqueuing elements does not require waiting, and is not fallible.
///         q.enqueue(Box::pin(Entry::new(1)));
///         q.enqueue(Box::pin(Entry::new(2)));
///     }
/// });
///
/// thread::spawn({
///     let q = q.clone();
///     move || {
///         q.enqueue(Box::pin(Entry::new(3)));
///         q.enqueue(Box::pin(Entry::new(4)));
///     }
/// });
///
///
/// // Dequeue elements until the producer threads have terminated.
/// let mut seen = Vec::new();
/// loop {
///     // Make sure we run at least once, in case the producer is already done.
///     let done = Arc::strong_count(&q) == 1;
///
///     // Dequeue until the queue is empty.
///     while let Some(entry) = q.dequeue() {
///         seen.push(entry.as_ref().val);
///     }
///
///     // If there are still producers, we may continue dequeuing.
///     if done {
///         break;
///     }
///
///     thread::yield_now();
/// }
///
/// // The elements may not have been received in order, so sort the
/// // received values before making assertions about them.
/// &mut seen[..].sort();
///
/// assert_eq!(&[1, 2, 3, 4], &seen[..]);
/// ```
///
/// The [`Consumer`] type may be used to reserve the permission to consume
/// multiple elements at a time:
///
/// ```
/// # use cordyceps::{
/// #     Linked,
/// #     mpsc_queue::{self, MpscQueue},
/// # };
/// # use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
/// #
/// # #[derive(Debug, Default)]
/// # struct Entry {
/// #    links: mpsc_queue::Links<Entry>,
/// #    val: i32,
/// # }
/// #
/// # unsafe impl Linked<mpsc_queue::Links<Entry>> for Entry {
/// #     type Handle = Pin<Box<Self>>;
/// #
/// #     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
/// #        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
/// #     }
/// #
/// #     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
/// #         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
/// #     }
/// #
/// #     unsafe fn links(target: NonNull<Entry>) -> NonNull<mpsc_queue::Links<Entry>> {
/// #        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
/// #        NonNull::new_unchecked(links)
/// #     }
/// # }
/// #
/// # impl Entry {
/// #     fn new(val: i32) -> Self {
/// #         Self {
/// #             val,
/// #             ..Self::default()
/// #         }
/// #     }
/// # }
/// let stub = Box::pin(Entry::default());
/// let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));
///
/// thread::spawn({
///     let q = q.clone();
///     move || {
///         q.enqueue(Box::pin(Entry::new(1)));
///         q.enqueue(Box::pin(Entry::new(2)));
///     }
/// });
///
/// // Reserve exclusive permission to consume elements
/// let consumer = q.consume();
///
/// let mut seen = Vec::new();
/// loop {
///     // Make sure we run at least once, in case the producer is already done.
///     let done = Arc::strong_count(&q) == 1;
///
///     // Dequeue until the queue is empty.
///     while let Some(entry) = consumer.dequeue() {
///         seen.push(entry.as_ref().val);
///     }
///
///     if done {
///         break;
///     }
///     thread::yield_now();
/// }
///
/// assert_eq!(&[1, 2], &seen[..]);
/// ```
///
/// The [`Consumer`] type also implements [`Iterator`]:
///
/// ```
/// # use cordyceps::{
/// #     Linked,
/// #     mpsc_queue::{self, MpscQueue},
/// # };
/// # use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
/// #
/// # #[derive(Debug, Default)]
/// # struct Entry {
/// #    links: mpsc_queue::Links<Entry>,
/// #    val: i32,
/// # }
/// #
/// # unsafe impl Linked<mpsc_queue::Links<Entry>> for Entry {
/// #     type Handle = Pin<Box<Self>>;
/// #
/// #     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
/// #        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
/// #     }
/// #
/// #     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
/// #         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
/// #     }
/// #
/// #     unsafe fn links(target: NonNull<Entry>) -> NonNull<mpsc_queue::Links<Entry>> {
/// #        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
/// #        NonNull::new_unchecked(links)
/// #     }
/// # }
/// #
/// # impl Entry {
/// #     fn new(val: i32) -> Self {
/// #         Self {
/// #             val,
/// #             ..Self::default()
/// #         }
/// #     }
/// # }
/// let stub = Box::pin(Entry::default());
/// let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));
///
/// thread::spawn({
///     let q = q.clone();
///     move || {
///         for i in 1..5 {
///             q.enqueue(Box::pin(Entry::new(i)));
///         }
///     }
/// });
///
/// thread::spawn({
///     let q = q.clone();
///     move || {
///         for i in 5..=10 {
///             q.enqueue(Box::pin(Entry::new(i)));
///         }
///     }
/// });
///
/// let mut seen = Vec::new();
/// loop {
///     // Make sure we run at least once, in case the producer is already done.
///     let done = Arc::strong_count(&q) == 1;
///
///     // Append any elements currently in the queue to the `Vec`
///     seen.extend(q.consume().map(|entry| entry.as_ref().val));
///
///     if done {
///         break;
///     }
///
///     thread::yield_now();
/// }
///
/// &mut seen[..].sort();
/// assert_eq!(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &seen[..]);
/// ```
///
/// # Implementation Details
///
/// This queue design is conceptually very simple, and has *extremely* fast and
/// wait-free producers (the [`enqueue`] operation). Enqueuing an element always
/// performs exactly one atomic swap and one atomic store, so producers need
/// never wait.
///
/// The consumer (the [`dequeue`]) is *typically* wait-free in the common case,
/// but must occasionally wait when the queue is in an inconsistent state.
///
/// ## Inconsistent States
///
/// As discussed in the [algorithm description on 1024cores.net][vyukov], it
/// is possible for this queue design to enter an inconsistent state if the
/// consumer tries to dequeue an element while a producer is in the middle
/// of enqueueing a new element. This occurs when a producer is between the
/// atomic swap with the `head` of the queue and the atomic store that sets the
/// `next` pointer of the previous `head` element. When the queue is in an
/// inconsistent state, the consumer must briefly wait before dequeueing an
/// element.
///
/// The consumer's behavior in the inconsistent state depends on which API
/// method is used. The [`MpscQueue::dequeue`] and [`Consumer::dequeue`] methods
/// will wait by spinning (with an exponential backoff) when the queue is
/// inconsistent. Alternatively, the [`MpscQueue::try_dequeue`] and
/// [`Consumer::try_dequeue`] methods will instead return [an error] when the
/// queue is in an inconsistent state.
///
/// [intrusive]: crate#intrusive-data-structures
/// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
/// [`dequeue`]: Self::dequeue
/// [`enqueue`]: Self::enqueue
/// [an error]: TryDequeueError
pub struct MpscQueue<T: Linked<Links<T>>> {
    /// The head of the queue. This is accessed in both `enqueue` and `dequeue`.
    head: CachePadded<AtomicPtr<T>>,

    /// The tail of the queue. This is accessed only when dequeueing.
    tail: CachePadded<UnsafeCell<*mut T>>,

    /// Does a consumer handle to the queue exist? If not, it is safe to create a
    /// new consumer.
    has_consumer: CachePadded<AtomicBool>,

    /// If the stub node is in a `static`, we cannot drop it when the
    /// queue is dropped.
    stub_is_static: bool,

    stub: NonNull<T>,
}

/// A handle that holds the right to dequeue elements from a [`MpscQueue`].
///
/// This can be used when one thread wishes to dequeue many elements at a time,
/// to avoid the overhead of ensuring mutual exclusion on every [`dequeue`] or
/// [`try_dequeue`] call.
///
/// This type is returned by the [`MpscQueue::consume`] and [`MpscQueue::try_consume`]
/// methods.
///
/// If the right to dequeue elements needs to be reserved for longer than a
/// single scope, an owned variant ([`OwnedConsumer`]) is also available, when
/// the [`MpscQueue`] is stored in an [`Arc`]. Since the [`MpscQueue`] must be stored
/// in an [`Arc`], the [`OwnedConsumer`] type requires the "alloc" feature flag.
///
/// [`Arc`]: alloc::sync::Arc
/// [`dequeue`]: Consumer::dequeue
/// [`try_dequeue`]: Consumer::try_dequeue
pub struct Consumer<'q, T: Linked<Links<T>>> {
    q: &'q MpscQueue<T>,
}

/// Links to other nodes in a [`MpscQueue`].
///
/// In order to be part of a [`MpscQueue`], a type must contain an instance of this
/// type, and must implement the [`Linked`] trait for `Links<Self>`.
pub struct Links<T> {
    /// The next node in the queue.
    next: AtomicPtr<T>,

    /// Is this the stub node?
    ///
    /// Used for debug mode consistency checking only.
    #[cfg(debug_assertions)]
    is_stub: AtomicBool,

    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// <https://github.com/rust-lang/rust/issues/63818>.
    _unpin: PhantomPinned,
}

/// Errors returned by [`MpscQueue::try_dequeue`] and [`Consumer::try_dequeue`].
#[derive(Debug, Eq, PartialEq)]
pub enum TryDequeueError {
    /// No element was dequeued because the queue was empty.
    Empty,

    /// The queue is currently in an [inconsistent state].
    ///
    /// Since inconsistent states are very short-lived, the caller may want to
    /// try dequeueing a second time.
    ///
    /// [inconsistent state]: MpscQueue#inconsistent-states
    Inconsistent,

    /// Another thread is currently calling [`MpscQueue::try_dequeue`]  or
    /// [`MpscQueue::dequeue`], or owns a [`Consumer`] or [`OwnedConsumer`] handle.
    ///
    /// This is a multi-producer, *single-consumer* queue, so only a single
    /// thread may dequeue elements at any given time.
    Busy,
}

// === impl Queue ===

impl<T: Linked<Links<T>>> MpscQueue<T> {
    /// Returns a new `MpscQueue`.
    ///
    /// The [`Default`] implementation for `T::Handle` is used to produce a new
    /// node used as the list's stub.
    #[must_use]
    pub fn new() -> Self
    where
        T::Handle: Default,
    {
        Self::new_with_stub(Default::default())
    }

    /// Returns a new `MpscQueue` with the provided stub node.
    ///
    /// If a `MpscQueue` must be constructed in a `const` context, such as a
    /// `static` initializer, see [`MpscQueue::new_with_static_stub`].
    #[must_use]
    pub fn new_with_stub(stub: T::Handle) -> Self {
        let stub = T::into_ptr(stub);

        // In debug mode, set the stub flag for consistency checking.
        #[cfg(debug_assertions)]
        unsafe {
            links(stub).is_stub.store(true, Release);
        }
        let ptr = stub.as_ptr();

        Self {
            head: CachePadded(AtomicPtr::new(ptr)),
            tail: CachePadded(UnsafeCell::new(ptr)),
            has_consumer: CachePadded(AtomicBool::new(false)),
            stub_is_static: false,
            stub,
        }
    }

    /// Returns a new `MpscQueue` with a static "stub" entity
    ///
    /// This is primarily used for creating an `MpscQueue` as a `static` variable.
    ///
    /// # Usage notes
    ///
    /// Unlike [`MpscQueue::new`] or [`MpscQueue::new_with_stub`], the `stub`
    /// item will NOT be dropped when the `MpscQueue` is dropped. This is fine
    /// if you are ALSO statically creating the `stub`. However, if it is
    /// necessary to recover that memory after the `MpscQueue` has been dropped,
    /// that will need to be done by the user manually.
    ///
    /// # Safety
    ///
    /// The `stub` provided must ONLY EVER be used for a single `MpscQueue`
    /// instance. Re-using the stub for multiple queues may lead to undefined
    /// behavior.
    ///
    /// ## Example usage
    ///
    /// ```rust
    /// # use cordyceps::{
    /// #     Linked,
    /// #     mpsc_queue::{self, MpscQueue},
    /// # };
    /// # use std::{pin::Pin, ptr::{self, NonNull}, thread, sync::Arc};
    /// #
    /// #
    ///
    /// // This is our same `Entry` from the parent examples. It has implemented
    /// // the `Links` trait as above.
    /// #[derive(Debug, Default)]
    /// struct Entry {
    ///    links: mpsc_queue::Links<Entry>,
    ///    val: i32,
    /// }
    ///
    /// #
    /// # unsafe impl Linked<mpsc_queue::Links<Entry>> for Entry {
    /// #     type Handle = Pin<Box<Self>>;
    /// #
    /// #     fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
    /// #        unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
    /// #     }
    /// #
    /// #     unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
    /// #         Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
    /// #     }
    /// #
    /// #     unsafe fn links(target: NonNull<Entry>) -> NonNull<mpsc_queue::Links<Entry>> {
    /// #        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
    /// #        NonNull::new_unchecked(links)
    /// #     }
    /// # }
    /// #
    /// # impl Entry {
    /// #     fn new(val: i32) -> Self {
    /// #         Self {
    /// #             val,
    /// #             ..Self::default()
    /// #         }
    /// #     }
    /// # }
    ///
    ///
    /// static MPSC: MpscQueue<Entry> = {
    ///     static STUB_ENTRY: Entry = Entry {
    ///         links: mpsc_queue::Links::<Entry>::new_stub(),
    ///         val: 0
    ///     };
    ///
    ///     // SAFETY: The stub may not be used by another MPSC queue.
    ///     // Here, this is ensured because the `STUB_ENTRY` static is defined
    ///     // inside of the initializer for the `MPSC` static, so it cannot be referenced
    ///     // elsewhere.
    ///     unsafe { MpscQueue::new_with_static_stub(&STUB_ENTRY) }
    /// };
    /// ```
    ///
    #[cfg(not(loom))]
    #[must_use]
    pub const unsafe fn new_with_static_stub(stub: &'static T) -> Self {
        let ptr = stub as *const T as *mut T;
        Self {
            head: CachePadded(AtomicPtr::new(ptr)),
            tail: CachePadded(UnsafeCell::new(ptr)),
            has_consumer: CachePadded(AtomicBool::new(false)),
            stub_is_static: true,
            stub: NonNull::new_unchecked(ptr),
        }
    }

    /// Enqueue a new element at the end of the queue.
    ///
    /// This takes ownership of a [`Handle`] that owns the element, and
    /// (conceptually) assigns ownership of the element to the queue while it
    /// remains enqueued.
    ///
    /// This method will never wait.
    ///
    /// [`Handle`]: crate::Linked::Handle
    pub fn enqueue(&self, element: T::Handle) {
        let ptr = T::into_ptr(element);

        #[cfg(debug_assertions)]
        debug_assert!(!unsafe { T::links(ptr).as_ref() }.is_stub());

        self.enqueue_inner(ptr)
    }

    #[inline]
    fn enqueue_inner(&self, ptr: NonNull<T>) {
        unsafe { links(ptr).next.store(ptr::null_mut(), Relaxed) };

        let ptr = ptr.as_ptr();
        let prev = self.head.swap(ptr, AcqRel);
        unsafe {
            // Safety: in release mode, we don't null check `prev`. This is
            // because no pointer in the list should ever be a null pointer, due
            // to the presence of the stub node.
            links(non_null(prev)).next.store(ptr, Release);
        }
    }

    /// Try to dequeue an element from the queue, without waiting if the queue
    /// is in an [inconsistent state], or until there is no other consumer trying
    /// to read from the queue.
    ///
    /// Because this is a multi-producer, _single-consumer_ queue,
    /// only one thread may be dequeueing at a time. If another thread is
    /// dequeueing, this method returns [`TryDequeueError::Busy`].
    ///
    /// The [`MpscQueue::dequeue`] method will instead wait (by spinning with an
    /// exponential backoff) when the queue is in an inconsistent state or busy.
    ///
    /// The unsafe [`MpscQueue::try_dequeue_unchecked`] method will not check if the
    /// queue is busy before dequeueing an element. This can be used when the
    /// user code guarantees that no other threads will dequeue from the queue
    /// concurrently, but this cannot be enforced by the compiler.
    ///
    /// This method will never wait.
    ///
    /// # Returns
    ///
    /// - `Ok`([`T::Handle`]`)` if an element was successfully dequeued
    /// - `Err(`[`TryDequeueError::Empty`]`)` if there are no elements in the queue
    /// - `Err(`[`TryDequeueError::Inconsistent`]`)` if the queue is currently in an
    ///   inconsistent state
    /// - `Err(`[`TryDequeueError::Busy`]`)` if another thread is currently trying to
    ///   dequeue a message.
    ///
    /// [inconsistent state]: Self#inconsistent-states
    /// [`T::Handle`]: crate::Linked::Handle
    pub fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
        if self
            .has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            return Err(TryDequeueError::Busy);
        }

        let res = unsafe {
            // Safety: the `has_consumer` flag ensures mutual exclusion of
            // consumers.
            self.try_dequeue_unchecked()
        };

        self.has_consumer.store(false, Release);
        res
    }

    /// Dequeue an element from the queue.
    ///
    /// This method will wait by spinning with an exponential backoff if the
    /// queue is in an [inconsistent state].
    ///
    /// Additionally, because this is a multi-producer, _single-consumer_ queue,
    /// only one thread may be dequeueing at a time. If another thread is
    /// dequeueing, this method will spin until the queue is no longer busy.
    ///
    /// The [`MpscQueue::try_dequeue`] will return an error rather than waiting when
    /// the queue is in an inconsistent state or busy.
    ///
    /// The unsafe [`MpscQueue::dequeue_unchecked`] method will not check if the
    /// queue is busy before dequeueing an element. This can be used when the
    /// user code guarantees that no other threads will dequeue from the queue
    /// concurrently, but this cannot be enforced by the compiler.
    ///
    /// # Returns
    ///
    /// - `Some(`[`T::Handle`]`)` if an element was successfully dequeued
    /// - `None` if the queue is empty or another thread is dequeueing
    ///
    /// [inconsistent state]: Self#inconsistent-states
    /// [`T::Handle`]: crate::Linked::Handle
    pub fn dequeue(&self) -> Option<T::Handle> {
        let mut boff = Backoff::new();
        loop {
            match self.try_dequeue() {
                Ok(val) => return Some(val),
                Err(TryDequeueError::Empty) => return None,
                Err(_) => boff.spin(),
            }
        }
    }

    /// Returns a [`Consumer`] handle that reserves the exclusive right to dequeue
    /// elements from the queue until it is dropped.
    ///
    /// If another thread is dequeueing, this method spins until there is no
    /// other thread dequeueing.
    pub fn consume(&self) -> Consumer<'_, T> {
        self.lock_consumer();
        Consumer { q: self }
    }

    /// Attempts to reserve a [`Consumer`] handle that holds the exclusive right
    /// to dequeue  elements from the queue until it is dropped.
    ///
    /// If another thread is dequeueing, this returns `None` instead.
    pub fn try_consume(&self) -> Option<Consumer<'_, T>> {
        // lock the consumer-side of the queue.
        self.try_lock_consumer().map(|_| Consumer { q: self })
    }

    /// Try to dequeue an element from the queue, without waiting if the queue
    /// is in an inconsistent state, and without checking if another consumer
    /// exists.
    ///
    /// This method returns [`TryDequeueError::Inconsistent`] when the queue is
    /// in an [inconsistent state].
    ///
    /// The [`MpscQueue::dequeue_unchecked`] method will instead wait (by
    /// spinning with an  exponential backoff) when the queue is in an
    /// inconsistent state.
    ///
    /// This method will never wait.
    ///
    /// # Returns
    ///
    /// - `Ok`([`T::Handle`]`)` if an element was successfully dequeued
    /// - `Err(`[`TryDequeueError::Empty`]`)` if there are no elements in the queue
    /// - `Err(`[`TryDequeueError::Inconsistent`]`)` if the queue is currently in an
    ///   inconsistent state
    ///
    /// This method will **never** return [`TryDequeueError::Busy`].
    ///
    /// # Safety
    ///
    /// This is a multi-producer, *single-consumer* queue. Only one thread/core
    /// may call `try_dequeue_unchecked` at a time!
    ///
    /// [inconsistent state]: Self#inconsistent-states
    /// [`T::Handle`]: crate::Linked::Handle
    pub unsafe fn try_dequeue_unchecked(&self) -> Result<T::Handle, TryDequeueError> {
        self.tail.with_mut(|tail| {
            let mut tail_node = NonNull::new(*tail).ok_or(TryDequeueError::Empty)?;
            let mut next = links(tail_node).next.load(Acquire);

            if tail_node == self.stub {
                #[cfg(debug_assertions)]
                debug_assert!(links(tail_node).is_stub());
                let next_node = NonNull::new(next).ok_or(TryDequeueError::Empty)?;

                *tail = next;
                tail_node = next_node;
                next = links(next_node).next.load(Acquire);
            }

            if !next.is_null() {
                *tail = next;
                return Ok(T::from_ptr(tail_node));
            }

            let head = self.head.load(Acquire);

            if tail_node.as_ptr() != head {
                return Err(TryDequeueError::Inconsistent);
            }

            self.enqueue_inner(self.stub);

            next = links(tail_node).next.load(Acquire);
            if next.is_null() {
                return Err(TryDequeueError::Empty);
            }

            *tail = next;

            #[cfg(debug_assertions)]
            debug_assert!(!links(tail_node).is_stub());

            Ok(T::from_ptr(tail_node))
        })
    }

    /// Dequeue an element from the queue, without checking whether another
    /// consumer exists.
    ///
    /// This method will wait by spinning with an exponential backoff if the
    /// queue is in an [inconsistent state].
    ///
    /// The [`MpscQueue::try_dequeue`] will return an error rather than waiting
    /// when the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `Some(`[`T::Handle`]`)` if an element was successfully dequeued
    /// - `None` if the queue is empty
    ///
    /// # Safety
    ///
    /// This is a multi-producer, *single-consumer* queue. Only one thread/core
    /// may call `dequeue` at a time!
    ///
    /// [inconsistent state]: Self#inconsistent-states
    /// [`T::Handle`]: crate::Linked::Handle
    pub unsafe fn dequeue_unchecked(&self) -> Option<T::Handle> {
        let mut boff = Backoff::new();
        loop {
            match self.try_dequeue_unchecked() {
                Ok(val) => return Some(val),
                Err(TryDequeueError::Empty) => return None,
                Err(TryDequeueError::Inconsistent) => boff.spin(),
                Err(TryDequeueError::Busy) => {
                    unreachable!("try_dequeue_unchecked never returns `Busy`!")
                }
            }
        }
    }

    #[inline]
    fn lock_consumer(&self) {
        let mut boff = Backoff::new();
        while self
            .has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            while self.has_consumer.load(Relaxed) {
                boff.spin();
            }
        }
    }

    #[inline]
    fn try_lock_consumer(&self) -> Option<()> {
        self.has_consumer
            .compare_exchange(false, true, AcqRel, Acquire)
            .map(|_| ())
            .ok()
    }
}

impl<T: Linked<Links<T>>> Drop for MpscQueue<T> {
    fn drop(&mut self) {
        let mut current = self.tail.with_mut(|tail| unsafe {
            // Safety: because `Drop` is called with `&mut self`, we have
            // exclusive ownership over the queue, so it's always okay to touch
            // the tail cell.
            *tail
        });
        while let Some(node) = NonNull::new(current) {
            unsafe {
                let links = links(node);
                let next = links.next.load(Relaxed);

                // Skip dropping the stub node; it is owned by the queue and
                // will be dropped when the queue is dropped. If we dropped it
                // here, that would cause a double free!
                if node != self.stub {
                    // Convert the pointer to the owning handle and drop it.
                    #[cfg(debug_assertions)]
                    debug_assert!(!links.is_stub(), "stub: {:p}, node: {node:p}", self.stub);
                    drop(T::from_ptr(node));
                } else {
                    #[cfg(debug_assertions)]
                    debug_assert!(links.is_stub());
                }

                current = next;
            }
        }

        unsafe {
            // If the stub is static, don't drop it. It lives 5eva
            // (that's one more than 4eva)
            if !self.stub_is_static {
                drop(T::from_ptr(self.stub));
            }
        }
    }
}

impl<T> fmt::Debug for MpscQueue<T>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            head,
            tail: _,
            has_consumer,
            stub,
            stub_is_static,
        } = self;
        f.debug_struct("MpscQueue")
            .field("head", &format_args!("{:p}", head.load(Acquire)))
            // only the consumer can load the tail; trying to print it here
            // could be racy.
            // XXX(eliza): we could replace the `UnsafeCell` with an atomic,
            // and then it would be okay to print the tail...but then we would
            // lose loom checking for tail accesses...
            .field("tail", &format_args!("..."))
            .field("has_consumer", &has_consumer.load(Acquire))
            .field("stub", stub)
            .field("stub_is_static", stub_is_static)
            .finish()
    }
}

impl<T> Default for MpscQueue<T>
where
    T: Linked<Links<T>>,
    T::Handle: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T> Send for MpscQueue<T>
where
    T: Send + Linked<Links<T>>,
    T::Handle: Send,
{
}
unsafe impl<T: Send + Linked<Links<T>>> Sync for MpscQueue<T> {}

// === impl Consumer ===

impl<'q, T: Send + Linked<Links<T>>> Consumer<'q, T> {
    /// Dequeue an element from the queue.
    ///
    /// This method will wait by spinning with an exponential backoff if the
    /// queue is in an [inconsistent state].
    ///
    /// The [`Consumer::try_dequeue`] will return an error rather than waiting when
    /// the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `Some(`[`T::Handle`]`)` if an element was successfully dequeued
    /// - `None` if the queue is empty
    ///
    /// [inconsistent state]: Self#inconsistent-states
    /// [`T::Handle`]: crate::Linked::Handle
    #[inline]
    pub fn dequeue(&self) -> Option<T::Handle> {
        debug_assert!(self.q.has_consumer.load(Acquire));
        unsafe {
            // Safety: we have reserved exclusive access to the queue.
            self.q.dequeue_unchecked()
        }
    }

    /// Try to dequeue an element from the queue, without waiting if the queue
    /// is in an inconsistent state.
    ///
    /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
    /// is possible for this queue design to enter an inconsistent state if the
    /// consumer tries to dequeue an element while a producer is in the middle
    /// of enqueueing a new element. If this occurs, the consumer must briefly
    /// wait before dequeueing an element. This method returns
    /// [`TryDequeueError::Inconsistent`] when the queue is in an inconsistent
    /// state.
    ///
    /// The [`Consumer::dequeue`] method will instead wait (by spinning with an
    /// exponential backoff) when the queue is in an inconsistent state.
    ///
    /// # Returns
    ///
    /// - `T::Handle` if an element was successfully dequeued
    /// - [`TryDequeueError::Empty`] if there are no elements in the queue
    /// - [`TryDequeueError::Inconsistent`] if the queue is currently in an
    ///   inconsistent state
    ///
    ///
    /// # Returns
    ///
    /// - `Some(T::Handle)` if an element was successfully dequeued
    /// - `None` if the queue is empty
    ///
    /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
    #[inline]
    pub fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
        debug_assert!(self.q.has_consumer.load(Acquire));
        unsafe {
            // Safety: we have reserved exclusive access to the queue.
            self.q.try_dequeue_unchecked()
        }
    }
}

impl<T: Linked<Links<T>>> Drop for Consumer<'_, T> {
    fn drop(&mut self) {
        self.q.has_consumer.store(false, Release);
    }
}

impl<T> fmt::Debug for Consumer<'_, T>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { q } = self;
        let tail = q.tail.with(|tail| unsafe {
            // Safety: it's okay for the consumer to access the tail cell, since
            // we have exclusive access to it.
            *tail
        });
        f.debug_struct("Consumer")
            .field("q", &q)
            .field("tail", &tail)
            .finish()
    }
}

impl<T> Iterator for Consumer<'_, T>
where
    T: Send + Linked<Links<T>>,
{
    type Item = T::Handle;

    fn next(&mut self) -> Option<Self::Item> {
        self.dequeue()
    }
}

// === impl Links ===

impl<T> Links<T> {
    /// Returns a new set of `Links` for a [`MpscQueue`].
    #[cfg(not(loom))]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: AtomicBool::new(false),
        }
    }

    /// Returns a new set of `Links` for the stub node in an [`MpscQueue`].
    #[cfg(not(loom))]
    #[must_use]
    pub const fn new_stub() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: AtomicBool::new(true),
        }
    }

    /// Returns a new set of `Links` for a [`MpscQueue`].
    #[cfg(loom)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: AtomicBool::new(false),
        }
    }

    /// Returns a new set of `Links` for the stub node in an [`MpscQueue`].
    #[cfg(loom)]
    #[must_use]
    pub fn new_stub() -> Self {
        Self {
            next: AtomicPtr::new(ptr::null_mut()),
            _unpin: PhantomPinned,
            #[cfg(debug_assertions)]
            is_stub: AtomicBool::new(true),
        }
    }

    #[cfg(debug_assertions)]
    fn is_stub(&self) -> bool {
        self.is_stub.load(Acquire)
    }
}

impl<T> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> fmt::Debug for Links<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Links");
        s.field("next", &self.next.load(Acquire));
        #[cfg(debug_assertions)]
        s.field("is_stub", &self.is_stub.load(Acquire));
        s.finish()
    }
}

feature! {
    #![feature = "alloc"]

    use alloc::sync::Arc;

    /// An owned handle that holds the right to dequeue elements from the queue.
    ///
    /// This can be used when one thread wishes to dequeue many elements at a time,
    /// to avoid the overhead of ensuring mutual exclusion on every [`dequeue`] or
    /// [`try_dequeue`] call.
    ///
    /// This type is returned by the [`MpscQueue::consume_owned`] and
    /// [`MpscQueue::try_consume_owned`] methods.
    ///
    /// This is similar to the [`Consumer`] type, but the queue is stored in an
    /// [`Arc`] rather than borrowed. This allows a single `OwnedConsumer`
    /// instance to be stored in a struct and used indefinitely.
    ///
    /// Since the queue is stored in an [`Arc`], this requires the `alloc`
    /// feature flag to be enabled.
    ///
    /// [`dequeue`]: OwnedConsumer::dequeue
    /// [`try_dequeue`]: OwnedConsumer::try_dequeue
    /// [`Arc`]: alloc::sync::Arc
    pub struct OwnedConsumer<T: Linked<Links<T>>> {
        q: Arc<MpscQueue<T>>
    }

    // === impl Consumer ===

    impl<T: Linked<Links<T>>> OwnedConsumer<T> {
        /// Dequeue an element from the queue.
        ///
        /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
        /// is possible for this queue design to enter an inconsistent state if the
        /// consumer tries to dequeue an element while a producer is in the middle
        /// of enqueueing a new element. If this occurs, the consumer must briefly
        /// wait before dequeueing an element. This method will wait by spinning
        /// with an exponential backoff if the queue is in an inconsistent state.
        ///
        /// The [`Consumer::try_dequeue`] will return an error rather than waiting when
        /// the queue is in an inconsistent state.
        ///
        /// # Returns
        ///
        /// - `Some(T::Handle)` if an element was successfully dequeued
        /// - `None` if the queue is empty
        ///
        /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
        #[inline]
        pub fn dequeue(&self) -> Option<T::Handle> {
            debug_assert!(self.q.has_consumer.load(Acquire));
            unsafe {
                // Safety: we have reserved exclusive access to the queue.
                self.q.dequeue_unchecked()
            }
        }

        /// Try to dequeue an element from the queue, without waiting if the queue
        /// is in an inconsistent state.
        ///
        /// As discussed in the [algorithm description on 1024cores.net][vyukov], it
        /// is possible for this queue design to enter an inconsistent state if the
        /// consumer tries to dequeue an element while a producer is in the middle
        /// of enqueueing a new element. If this occurs, the consumer must briefly
        /// wait before dequeueing an element. This method returns
        /// [`TryDequeueError::Inconsistent`] when the queue is in an inconsistent
        /// state.
        ///
        /// The [`Consumer::dequeue`] method will instead wait (by spinning with an
        /// exponential backoff) when the queue is in an inconsistent state.
        ///
        /// # Returns
        ///
        /// - `T::Handle` if an element was successfully dequeued
        /// - [`TryDequeueError::Empty`] if there are no elements in the queue
        /// - [`TryDequeueError::Inconsistent`] if the queue is currently in an
        ///   inconsistent state
        ///
        ///
        /// # Returns
        ///
        /// - `Some(T::Handle)` if an element was successfully dequeued
        /// - `None` if the queue is empty
        ///
        /// [vyukov]: http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue
        #[inline]
        pub fn try_dequeue(&self) -> Result<T::Handle, TryDequeueError> {
            debug_assert!(self.q.has_consumer.load(Acquire));
            unsafe {
                // Safety: we have reserved exclusive access to the queue.
                self.q.try_dequeue_unchecked()
            }
        }

        /// Returns `true` if any producers exist for this queue.
        pub fn has_producers(&self) -> bool {
            Arc::strong_count(&self.q) > 1
        }
    }

    impl<T: Linked<Links<T>>> Drop for OwnedConsumer<T> {
        fn drop(&mut self) {
            self.q.has_consumer.store(false, Release);
        }
    }

    impl<T: Linked<Links<T>>> fmt::Debug for OwnedConsumer<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self { q } = self;
            let tail = q.tail.with(|tail| unsafe {
                // Safety: it's okay for the consumer to access the tail cell, since
                // we have exclusive access to it.
                *tail
            });
            f.debug_struct("OwnedConsumer")
                .field("q", &q)
                .field("tail", &tail)
                .finish()
        }
    }

    // === impl Queue ===

    impl<T: Linked<Links<T>>> MpscQueue<T> {
        /// Returns a [`OwnedConsumer`] handle that reserves the exclusive right to dequeue
        /// elements from the queue until it is dropped.
        ///
        /// If another thread is dequeueing, this method spins until there is no
        /// other thread dequeueing.
        pub fn consume_owned(self: Arc<Self>) -> OwnedConsumer<T> {
            self.lock_consumer();
            OwnedConsumer { q: self }
        }

        /// Attempts to reserve an [`OwnedConsumer`] handle that holds the exclusive right
        /// to dequeue  elements from the queue until it is dropped.
        ///
        /// If another thread is dequeueing, this returns `None` instead.
        pub fn try_consume_owned(self: Arc<Self>) -> Option<OwnedConsumer<T>> {
            self.try_lock_consumer().map(|_| OwnedConsumer { q: self })
        }
    }
}

/// Just a little helper so we don't have to add `.as_ref()` noise everywhere...
#[inline(always)]
unsafe fn links<'a, T: Linked<Links<T>>>(ptr: NonNull<T>) -> &'a Links<T> {
    T::links(ptr).as_ref()
}

/// Helper to construct a `NonNull<T>` from a raw pointer to `T`, with null
/// checks elided in release mode.
#[cfg(debug_assertions)]
#[track_caller]
#[inline(always)]
unsafe fn non_null<T>(ptr: *mut T) -> NonNull<T> {
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

#[cfg(all(loom, test))]
mod loom {
    use super::*;
    use crate::loom::{self, sync::Arc, thread};
    use test_util::*;

    #[test]
    fn basically_works_loom() {
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS;
        const TOTAL_MSGS: i32 = THREADS * MSGS;
        basically_works_test(THREADS, MSGS, TOTAL_MSGS);
    }

    #[test]
    fn doesnt_leak() {
        // Test that dropping the queue drops any messages that haven't been
        // consumed by the consumer.
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS;
        // Only consume half as many messages as are sent, to ensure dropping
        // the queue does not leak.
        const TOTAL_MSGS: i32 = (THREADS * MSGS) / 2;
        basically_works_test(THREADS, MSGS, TOTAL_MSGS);
    }

    fn basically_works_test(threads: i32, msgs: i32, total_msgs: i32) {
        loom::model(move || {
            let stub = entry(666);
            let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));

            let threads: Vec<_> = (0..threads)
                .map(|thread| thread::spawn(do_tx(thread, msgs, &q)))
                .collect();

            let mut i = 0;
            while i < total_msgs {
                match q.try_dequeue() {
                    Ok(val) => {
                        i += 1;
                        tracing::info!(?val, "dequeue {}/{}", i, total_msgs);
                    }
                    Err(TryDequeueError::Busy) => panic!(
                        "the queue should never be busy, as there is only a single consumer!"
                    ),
                    Err(err) => {
                        tracing::info!(?err, "dequeue error");
                        thread::yield_now();
                    }
                }
            }

            for thread in threads {
                thread.join().unwrap();
            }
        })
    }

    fn do_tx(thread: i32, msgs: i32, q: &Arc<MpscQueue<Entry>>) -> impl FnOnce() + Send + Sync {
        let q = q.clone();
        move || {
            for i in 0..msgs {
                q.enqueue(entry(i + (thread * 10)));
                tracing::info!(thread, "enqueue msg {}/{}", i, msgs);
            }
        }
    }

    #[test]
    fn mpmc() {
        // Tests multiple consumers competing for access to the consume side of
        // the queue.
        const THREADS: i32 = 2;
        const MSGS: i32 = THREADS;

        fn do_rx(thread: i32, q: Arc<MpscQueue<Entry>>) {
            let mut i = 0;
            while let Some(val) = q.dequeue() {
                tracing::info!(?val, ?thread, "dequeue {}/{}", i, THREADS * MSGS);
                i += 1;
            }
        }

        loom::model(|| {
            let stub = entry(666);
            let q = Arc::new(MpscQueue::<Entry>::new_with_stub(stub));

            let mut threads: Vec<_> = (0..THREADS)
                .map(|thread| thread::spawn(do_tx(thread, MSGS, &q)))
                .collect();

            threads.push(thread::spawn({
                let q = q.clone();
                move || do_rx(THREADS + 1, q)
            }));
            do_rx(THREADS + 2, q);

            for thread in threads {
                thread.join().unwrap();
            }
        })
    }

    #[test]
    fn crosses_queues() {
        loom::model(|| {
            let stub1 = entry(666);
            let q1 = Arc::new(MpscQueue::<Entry>::new_with_stub(stub1));

            let thread = thread::spawn({
                let q1 = q1.clone();
                move || {
                    let stub2 = entry(420);
                    let q2 = Arc::new(MpscQueue::<Entry>::new_with_stub(stub2));
                    // let mut dequeued = false;
                    for entry in q1.consume() {
                        tracing::info!("dequeued");
                        q2.enqueue(entry);
                        q2.try_dequeue().unwrap();
                    }
                    tracing::info!("consumer done\nq1={q1:#?}\nq2={q2:#?}");
                }
            });

            q1.enqueue(entry(1));
            drop(q1);

            thread.join().unwrap();
        })
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use test_util::*;

    use std::{ops::Deref, println, sync::Arc, thread};

    #[test]
    fn dequeue_empty() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);
        assert_eq!(q.dequeue(), None)
    }

    #[test]
    fn try_dequeue_empty() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);
        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Empty))
    }

    #[test]
    fn try_dequeue_busy() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);

        let consumer = q.try_consume().expect("must acquire consumer");
        assert_eq!(consumer.try_dequeue(), Err(TryDequeueError::Empty));

        q.enqueue(entry(1));

        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Busy));

        assert_eq!(consumer.try_dequeue(), Ok(entry(1)),);

        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Busy));

        assert_eq!(consumer.try_dequeue(), Err(TryDequeueError::Empty));

        drop(consumer);
        assert_eq!(q.try_dequeue(), Err(TryDequeueError::Empty));
    }

    #[test]
    fn enqueue_dequeue() {
        let stub = entry(666);
        let e = entry(1);
        let q = MpscQueue::<Entry>::new_with_stub(stub);
        q.enqueue(e);
        assert_eq!(q.dequeue(), Some(entry(1)));
        assert_eq!(q.dequeue(), None)
    }

    #[test]
    fn basically_works() {
        let stub = entry(666);
        let q = MpscQueue::<Entry>::new_with_stub(stub);

        let q = Arc::new(q);
        test_basically_works(q);
    }

    #[test]
    fn basically_works_all_const() {
        static STUB_ENTRY: Entry = const_stub_entry(666);
        static MPSC: MpscQueue<Entry> =
            unsafe { MpscQueue::<Entry>::new_with_static_stub(&STUB_ENTRY) };
        test_basically_works(&MPSC);
    }

    #[test]
    fn basically_works_mixed_const() {
        static STUB_ENTRY: Entry = const_stub_entry(666);
        let q = unsafe { MpscQueue::<Entry>::new_with_static_stub(&STUB_ENTRY) };

        let q = Arc::new(q);
        test_basically_works(q)
    }

    fn test_basically_works<Q>(q: Q)
    where
        Q: Deref<Target = MpscQueue<Entry>> + Clone,
        Q: Send + 'static,
    {
        const THREADS: i32 = if_miri(3, 8);
        const MSGS: i32 = if_miri(10, 1000);

        assert_eq!(q.dequeue(), None);

        let threads: Vec<_> = (0..THREADS)
            .map(|thread| {
                let q = q.clone();
                thread::spawn(move || {
                    for i in 0..MSGS {
                        q.enqueue(entry(i));
                        println!("thread {thread}; msg {i}/{MSGS}");
                    }
                })
            })
            .collect();

        let mut i = 0;
        while i < THREADS * MSGS {
            match q.try_dequeue() {
                Ok(msg) => {
                    i += 1;
                    println!("recv {msg:?} ({i}/{})", THREADS * MSGS);
                }
                Err(TryDequeueError::Busy) => {
                    panic!("the queue should never be busy, as there is only one consumer")
                }
                Err(e) => {
                    println!("recv error {e:?}");
                    thread::yield_now();
                }
            }
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }

    const fn if_miri(miri: i32, not_miri: i32) -> i32 {
        if cfg!(miri) {
            miri
        } else {
            not_miri
        }
    }
}

#[cfg(test)]
mod test_util {
    use super::*;
    use crate::loom::alloc;
    pub use std::{boxed::Box, pin::Pin, println, ptr, vec, vec::Vec};

    pub(super) struct Entry {
        links: Links<Entry>,
        pub(super) val: i32,
        // participate in loom leak checking
        _track: alloc::Track<()>,
    }

    impl std::cmp::PartialEq for Entry {
        fn eq(&self, other: &Self) -> bool {
            self.val == other.val
        }
    }

    unsafe impl Linked<Links<Self>> for Entry {
        type Handle = Pin<Box<Entry>>;

        fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Entry> {
            unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
        }

        unsafe fn from_ptr(ptr: NonNull<Entry>) -> Pin<Box<Entry>> {
            // Safety: if this function is only called by the linked list
            // implementation (and it is not intended for external use), we can
            // expect that the `NonNull` was constructed from a reference which
            // was pinned.
            //
            // If other callers besides `List`'s internals were to call this on
            // some random `NonNull<Entry>`, this would not be the case, and
            // this could be constructing an erroneous `Pin` from a referent
            // that may not be pinned!
            Pin::new_unchecked(Box::from_raw(ptr.as_ptr()))
        }

        unsafe fn links(target: NonNull<Entry>) -> NonNull<Links<Entry>> {
            let links = ptr::addr_of_mut!((*target.as_ptr()).links);
            NonNull::new_unchecked(links)
        }
    }

    impl fmt::Debug for Entry {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self { links, val, _track } = self;
            f.debug_struct("Entry")
                .field("links", links)
                .field("val", val)
                .field("_track", _track)
                .finish()
        }
    }

    #[cfg(not(loom))]
    pub(super) const fn const_stub_entry(val: i32) -> Entry {
        Entry {
            links: Links::new_stub(),
            val,
            _track: alloc::Track::new_const(()),
        }
    }

    pub(super) fn entry(val: i32) -> Pin<Box<Entry>> {
        Box::pin(Entry {
            links: Links::new(),
            val,
            _track: alloc::Track::new(()),
        })
    }
}
