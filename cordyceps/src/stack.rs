//! [Intrusive], singly-linked first-in, first-out (FIFO) stacks.
//!
//! See the documentation for the [`Stack`] and [`TransferStack`] types for
//! details.
//!
//! [intrusive]: crate#intrusive-data-structures
#![warn(missing_debug_implementations)]

use crate::{loom::cell::UnsafeCell, Linked};

pub use crate::singly::DrainFilter;

/// A cursor over a [`Stack`], which walks it from the front.
///
/// This is returned by [`Stack::cursor_front_mut`]; see that method for
/// details. Like [`List`]'s cursor, it has a position off the stack, where it
/// holds no element and from which moving on arrives back at the front.
///
/// [`List`]: crate::List
#[derive(Debug)]
pub struct CursorMut<'list, T: Linked<Links<T>>> {
    inner: crate::singly::CursorMut<'list, T>,
}

impl<T: Linked<Links<T>>> CursorMut<'_, T> {
    /// Returns how far the cursor is from the front, or `None` when it is off
    /// the stack.
    #[must_use]
    pub fn index(&self) -> Option<usize> {
        self.inner.index()
    }

    /// Borrows the element the cursor is on, or `None` when it is off the
    /// stack.
    #[must_use]
    pub fn current(&self) -> Option<Pin<&T>> {
        self.inner.current()
    }

    /// Borrows the element after the one the cursor is on. Off the stack, that
    /// is the front.
    #[must_use]
    pub fn peek_next(&self) -> Option<Pin<&T>> {
        self.inner.peek_next()
    }

    /// Moves the cursor to the next element.
    ///
    /// From the last element this moves off the stack; from off the stack it
    /// moves to the front, as [`List`]'s cursor does.
    ///
    /// [`List`]: crate::List
    pub fn move_next(&mut self) {
        self.inner.move_next()
    }

    /// Unlinks the element the cursor is on and returns the [`Handle`] owning
    /// it, leaving the cursor on whatever followed.
    ///
    /// Returns `None`, taking nothing, when the cursor is off the stack. What
    /// comes back is detached, so it can go straight into another collection.
    ///
    /// [`Handle`]: crate::Linked::Handle
    pub fn remove_current(&mut self) -> Option<T::Handle> {
        self.inner.remove_current()
    }

    /// Inserts `element` after the one the cursor is on, leaving the cursor
    /// where it was.
    ///
    /// Off the stack, the element goes to the front, as [`List`]'s cursor does.
    ///
    /// [`List`]: crate::List
    pub fn insert_after(&mut self, element: T::Handle) {
        self.inner.insert_after(element)
    }

    /// Splits the stack after the element the cursor is on, returning
    /// everything after it and leaving that element and those before it here.
    ///
    /// Off the stack, every element is moved.
    #[must_use]
    pub fn split_after(&mut self) -> Stack<T> {
        let (head, len) = self.inner.split_after();
        Stack { head, len }
    }
}
use core::{fmt, marker::PhantomPinned, pin::Pin, ptr::NonNull};

#[cfg(target_has_atomic = "ptr")]
pub use has_cas_atomics::*;

/// Items exclusive to targets with CAS atomics
#[cfg(target_has_atomic = "ptr")]
mod has_cas_atomics {
    use core::{
        fmt,
        ptr::{self, NonNull},
    };

    use crate::{
        loom::sync::atomic::{AtomicPtr, Ordering::*},
        Linked,
    };

    use super::{Links, Stack};

    /// An [intrusive] lock-free singly-linked FIFO stack, where all entries
    /// currently in the stack are consumed in a single atomic operation.
    ///
    /// A transfer stack is perhaps the world's simplest lock-free concurrent data
    /// structure. It provides two primary operations:
    ///
    /// - [`TransferStack::push`], which appends an element to the end of the
    ///   transfer stack,
    ///
    /// - [`TransferStack::take_all`], which atomically takes all elements currently
    ///   on the transfer stack and returns them as a new mutable [`Stack`].
    ///
    /// `push` is an *O*(1) operation, although it performs a compare-and-swap
    /// loop that may be retried if another producer concurrently pushed an
    /// element. `take_all` detaches every element in a single *O*(1) swap and
    /// then walks them once to give the returned [`Stack`] its length, so it
    /// is *O*(*n*) in the number of elements taken.
    ///
    /// In order to be part of a `TransferStack`, a type `T` must implement
    /// the [`Linked`] trait for [`stack::Links<T>`](Links).
    ///
    /// Pushing elements into a `TransferStack` takes ownership of those elements
    /// through an owning [`Handle` type](Linked::Handle). Dropping a
    /// [`TransferStack`] drops all elements currently linked into the stack.
    ///
    /// A transfer stack is often useful in cases where a large number of resources
    /// must be efficiently transferred from several producers to a consumer, such
    /// as for reuse or cleanup. For example, a [`TransferStack`] can be used as the
    /// "thread" (shared) free list in a [`mimalloc`-style sharded
    /// allocator][mimalloc], with a mutable [`Stack`] used as the local
    /// (unsynchronized) free list. When an allocation is freed from the same CPU
    /// core that it was allocated on, it is pushed to the local free list, using an
    /// unsynchronized mutable [`Stack::push`] operation. If an allocation is freed
    /// from a different thread, it is instead pushed to that thread's shared free
    /// list, a [`TransferStack`], using an atomic [`TransferStack::push`]
    /// operation. New allocations are popped from the local unsynchronized free
    /// list, and if the local free list is empty, the entire shared free list is
    /// moved onto the local free list. This allows objects which do not leave the
    /// CPU core they were allocated on to be both allocated and deallocated using
    /// unsynchronized operations, and new allocations only perform an atomic
    /// operation when the local free list is empty.
    ///
    /// [intrusive]: crate#intrusive-data-structures
    /// [mimalloc]: https://www.microsoft.com/en-us/research/uploads/prod/2019/06/mimalloc-tr-v1.pdf
    pub struct TransferStack<T: Linked<Links<T>>> {
        head: AtomicPtr<T>,
    }

    // === impl TransferStack ===
    impl<T> TransferStack<T>
    where
        T: Linked<Links<T>>,
    {
        /// Returns a new `TransferStack` with no elements.
        #[cfg(not(loom))]
        #[must_use]
        pub const fn new() -> Self {
            Self {
                head: AtomicPtr::new(ptr::null_mut()),
            }
        }

        /// Returns a new `TransferStack` with no elements.
        #[cfg(loom)]
        #[must_use]
        pub fn new() -> Self {
            Self {
                head: AtomicPtr::new(ptr::null_mut()),
            }
        }

        /// Pushes `element` onto the end of this `TransferStack`, taking ownership
        /// of it.
        ///
        /// This is an *O*(1) operation, although it performs a compare-and-swap
        /// loop that may repeat if another producer is concurrently calling `push`
        /// on the same `TransferStack`.
        ///
        /// This takes ownership over `element` through its [owning `Handle`
        /// type](Linked::Handle). If the `TransferStack` is dropped before the
        /// pushed `element` is removed from the stack, the `element` will be dropped.
        #[inline]
        pub fn push(&self, element: T::Handle) {
            self.push_was_empty(element);
        }

        /// Pushes `element` onto the end of this `TransferStack`, taking ownership
        /// of it. Returns `true` if the stack was previously empty (the previous
        /// head was null).
        ///
        /// This is an *O*(1) operation, although it performs a compare-and-swap
        /// loop that may repeat if another producer is concurrently calling `push`
        /// on the same `TransferStack`.
        ///
        /// This takes ownership over `element` through its [owning `Handle`
        /// type](Linked::Handle). If the `TransferStack` is dropped before the
        /// pushed `element` is removed from the stack, the `element` will be dropped.
        pub fn push_was_empty(&self, element: T::Handle) -> bool {
            let ptr = T::into_ptr(element);
            test_trace!(?ptr, "TransferStack::push");
            let links = unsafe { T::links(ptr).as_mut() };
            debug_assert!(links.next.with(|next| unsafe { (*next).is_none() }));

            let mut head = self.head.load(Relaxed);
            loop {
                test_trace!(?ptr, ?head, "TransferStack::push");
                links.next.with_mut(|next| unsafe {
                    *next = NonNull::new(head);
                });

                match self
                    .head
                    .compare_exchange_weak(head, ptr.as_ptr(), AcqRel, Acquire)
                {
                    Ok(old) => {
                        let was_empty = old.is_null();
                        test_trace!(?ptr, ?head, was_empty, "TransferStack::push -> pushed");
                        return was_empty;
                    }
                    Err(actual) => head = actual,
                }
            }
        }

        /// Takes all elements *currently* in this `TransferStack`, returning a new
        /// mutable [`Stack`] containing those elements.
        ///
        /// Taking the elements is a single atomic swap, which neither loops nor
        /// spins. The chain arrives without a count, so the elements are walked
        /// once to give the returned [`Stack`] its length, making this *O*(*n*)
        /// in the number of elements taken. It does not allocate memory.
        #[must_use]
        pub fn take_all(&self) -> Stack<T> {
            let head = self.head.swap(ptr::null_mut(), AcqRel);
            let head = NonNull::new(head);
            // the chain came off the lock-free stack without a count,
            // so there is nowhere cheaper to get one than by walking it.
            Stack {
                len: unsafe { crate::singly::count(head) },
                head,
            }
        }

        /// Returns `true` if this `TransferStack` is empty.
        ///
        /// Unlike [`take_all`], this only loads the head pointer, so a consumer
        /// polling a usually-empty stack can skip the read-modify-write. The
        /// load is `Relaxed` and the result is a snapshot: a producer may
        /// [`push`] the instant it returns, and a `false` result still has to be
        /// followed by [`take_all`], whose `AcqRel` swap is what synchronizes
        /// with the pushes it takes.
        ///
        /// [`take_all`]: Self::take_all
        /// [`push`]: Self::push
        #[inline]
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.head.load(Relaxed).is_null()
        }
    }

    impl<T> Drop for TransferStack<T>
    where
        T: Linked<Links<T>>,
    {
        fn drop(&mut self) {
            // The stack owns any entries that are still in the stack; ensure they
            // are dropped before dropping the stack.
            for entry in self.take_all() {
                drop(entry);
            }
        }
    }

    impl<T> fmt::Debug for TransferStack<T>
    where
        T: Linked<Links<T>>,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self { head } = self;
            f.debug_struct("TransferStack").field("head", head).finish()
        }
    }

    impl<T> Default for TransferStack<T>
    where
        T: Linked<Links<T>>,
    {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// An [intrusive] singly-linked mutable FIFO stack.
///
/// This is a very simple implementation of a linked `Stack`, which provides
/// *O*(1) [`push`](Self::push) and [`pop`](Self::pop) operations. Items are
/// popped from the stack in the opposite order that they were pushed in.
///
/// A [`Stack`] also implements the [`Iterator`] trait, with the
/// [`Iterator::next`] method popping elements from the end of the stack.
///
/// In order to be part of a `Stack`, a type `T` must implement
/// the [`Linked`] trait for [`stack::Links<T>`](Links).
///
/// Pushing elements into a `Stack` takes ownership of those elements
/// through an owning [`Handle` type](Linked::Handle). Dropping a
/// `Stack` drops all elements currently linked into the stack.
///
/// [intrusive]: crate#intrusive-data-structures
pub struct Stack<T: Linked<Links<T>>> {
    pub(crate) head: Option<NonNull<T>>,
    /// The number of elements linked into this stack.
    pub(crate) len: usize,
}

/// Singly-linked-list linkage
///
/// Links to other nodes in a [`TransferStack`], [`Stack`], or [`SortedList`].
///
/// In order to be part of a [`TransferStack`], [`Stack`], or [`SortedList`],
/// a type must contain an instance of this type, and must implement the
/// [`Linked`] trait for `Links<Self>`.
///
/// [`SortedList`]: crate::SortedList
//
// TODO(AJM): In the next breaking change, we might want to specifically have
// a `SingleLinks` and `DoubleLinks` type to make the relationship more clear,
// instead of "stack" being singly-flavored and "list" being doubly-flavored
pub struct Links<T> {
    /// The next node in the queue.
    pub(crate) next: UnsafeCell<Option<NonNull<T>>>,

    /// Linked list links must always be `!Unpin`, in order to ensure that they
    /// never recieve LLVM `noalias` annotations; see also
    /// <https://github.com/rust-lang/rust/issues/63818>.
    _unpin: PhantomPinned,
}

// === impl Stack ===

impl<T> Stack<T>
where
    T: Linked<Links<T>>,
{
    /// Returns a new `Stack` with no elements in it.
    #[must_use]
    pub const fn new() -> Self {
        Self { head: None, len: 0 }
    }

    /// Pushes `element` onto the end of this `Stack`, taking ownership
    /// of it.
    ///
    /// Returns `true` if the stack was previously empty, and `false` if the stack
    /// contained at least one other element.
    ///
    /// This is an *O*(1) operation that does not allocate memory. It will never
    /// loop.
    ///
    /// This takes ownership over `element` through its [owning `Handle`
    /// type](Linked::Handle). If the `Stack` is dropped before the
    /// pushed `element` is [`pop`](Self::pop)pped from the stack, the `element`
    /// will be dropped.
    pub fn push_was_empty(&mut self, element: T::Handle) -> bool {
        let ptr = T::into_ptr(element);
        self.len += 1;
        test_trace!(?ptr, ?self.head, "Stack::push_was_empty");
        unsafe {
            // Safety: we have exclusive mutable access to the stack, and
            // therefore can also mutate the stack's entries.
            let links = T::links(ptr).as_mut();
            links.next.with_mut(|next| {
                debug_assert!((*next).is_none());
                *next = self.head.replace(ptr);
                (*next).is_none()
            })
        }
    }

    /// Pushes `element` onto the end of this `Stack`, taking ownership
    /// of it.
    ///
    /// This is an *O*(1) operation that does not allocate memory. It will never
    /// loop.
    ///
    /// This takes ownership over `element` through its [owning `Handle`
    /// type](Linked::Handle). If the `Stack` is dropped before the
    /// pushed `element` is [`pop`](Self::pop)pped from the stack, the `element`
    /// will be dropped.
    ///
    /// For a variant of this method that returns a `bool` indicating if the
    /// list was empty, see [`Stack::push_was_empty`].
    #[inline]
    pub fn push(&mut self, element: T::Handle) {
        self.push_was_empty(element);
    }

    /// Returns the element most recently [push](Self::push)ed to this `Stack`,
    /// or `None` if the stack is empty.
    ///
    /// This is an *O*(1) operation which does not allocate memory. It will
    /// never loop and does not spin.
    #[must_use]
    pub fn pop(&mut self) -> Option<T::Handle> {
        test_trace!(?self.head, "Stack::pop");
        let head = self.head.take()?;
        self.len -= 1;
        unsafe {
            // Safety: we have exclusive ownership over this chunk of stack.

            // advance the head link to the next node after the current one (if
            // there is one).
            self.head = T::links(head).as_mut().next.with_mut(|next| (*next).take());

            test_trace!(?self.head, "Stack::pop -> popped");

            // return the current node
            Some(T::from_ptr(head))
        }
    }

    /// Takes all elements *currently* in this `Stack`, returning a new
    /// mutable `Stack` containing those elements.
    ///
    /// This is an *O*(1) operation which does not allocate memory. It will
    /// never loop and does not spin.
    #[must_use]
    pub fn take_all(&mut self) -> Self {
        Self {
            head: self.head.take(),
            len: core::mem::replace(&mut self.len, 0),
        }
    }

    /// Returns the number of elements in this `Stack`.
    ///
    /// This is an *O*(1) operation.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Borrows the element [`pop`](Self::pop) would return, without unlinking
    /// it, or `None` if the stack is empty.
    #[must_use]
    pub fn front(&self) -> Option<Pin<&T>> {
        let head = self.head?;
        // Safety: the stack owns this element, so it lives at least as long as
        // the borrow of `self`, and it is pinned for as long as it is linked.
        unsafe { Some(Pin::new_unchecked(head.as_ref())) }
    }

    /// Drops every element still linked into this `Stack`, leaving it empty.
    ///
    /// If an element's destructor panics, the remaining elements stay linked
    /// and the length continues to describe them.
    pub fn clear(&mut self) {
        // Safety: this stack owns every element in its chain and counts them.
        unsafe { crate::singly::clear(&mut self.head, &mut self.len) }
    }

    /// Unlinks `item` from this `Stack`, returning the [`Handle`] that owns it.
    ///
    /// # Returns
    ///
    /// - [`Some`]`(T::Handle)` owning `item`, if `item` was linked into this
    ///   stack.
    /// - [`None`] if `item` is not an element of this stack, in which case the
    ///   stack is left exactly as it was.
    ///
    /// Because the stack is singly linked, this walks it to find the element
    /// before `item`, and is therefore *O*(*n*).
    ///
    /// [`Handle`]: crate::Linked::Handle
    ///
    /// # Safety
    ///
    /// `item` must point to an element which has not been freed.
    pub unsafe fn remove(&mut self, item: NonNull<T>) -> Option<T::Handle> {
        if let Some(node) = crate::singly::unlink(&mut self.head, item) {
            self.len -= 1;
            Some(T::from_ptr(node))
        } else {
            None
        }
    }

    /// Returns an iterator which unlinks and yields the elements `pred`
    /// accepts, leaving the rest linked and in their existing order.
    ///
    /// Elements are handed to `pred` from the front. What the iterator yields
    /// is owned by the caller; dropping the iterator before it is exhausted
    /// leaves everything it has not reached linked, and the length correct.
    pub fn drain_filter<F>(&mut self, pred: F) -> DrainFilter<'_, T, F>
    where
        F: FnMut(&T) -> bool,
    {
        DrainFilter::new(&mut self.head, &mut self.len, pred)
    }

    /// Returns a cursor over this `Stack`, starting at the front.
    ///
    /// The cursor walks forwards only, which is all a singly-linked chain
    /// allows, and can look at the element it is on, take it out, put another
    /// one in after it, or split the stack there.
    #[must_use]
    pub fn cursor_front_mut(&mut self) -> CursorMut<'_, T> {
        CursorMut {
            inner: crate::singly::CursorMut::new(&mut self.head, &mut self.len),
        }
    }

    /// Asserts that this stack holds together: that its length is the number of
    /// elements actually linked into it.
    ///
    /// This is intended for tests and debug assertions.
    ///
    /// # Panics
    ///
    /// If the stack's length disagrees with its contents.
    #[track_caller]
    pub fn assert_valid(&self) {
        let mut walked = 0;
        let mut curr = self.head;
        while let Some(node) = curr {
            walked += 1;
            assert!(
                walked <= self.len,
                "stack holds more than the {} elements it claims; it may be cyclic",
                self.len
            );
            // Safety: `node` is a live element of this stack's chain.
            curr = unsafe { crate::singly::next_of(node) };
        }
        assert_eq!(
            walked, self.len,
            "stack claims {} elements but {walked} are linked into it",
            self.len
        );
    }

    /// Returns `true` if this `Stack` is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }
}

impl<T> Drop for Stack<T>
where
    T: Linked<Links<T>>,
{
    fn drop(&mut self) {
        // The stack owns any entries that are still in the stack; ensure they
        // are dropped before dropping the stack.
        for entry in self {
            drop(entry);
        }
    }
}

impl<T> fmt::Debug for Stack<T>
where
    T: Linked<Links<T>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { head, len } = self;
        f.debug_struct("Stack")
            .field("head", head)
            .field("len", len)
            .finish()
    }
}

impl<T> Iterator for Stack<T>
where
    T: Linked<Links<T>>,
{
    type Item = T::Handle;

    fn next(&mut self) -> Option<Self::Item> {
        self.pop()
    }
}

impl<T> Default for Stack<T>
where
    T: Linked<Links<T>>,
{
    fn default() -> Self {
        Self::new()
    }
}

/// # Safety
///
/// A `Stack` is `Send` if `T` is send, because moving it across threads
/// also implicitly moves any `T`s in the stack.
unsafe impl<T> Send for Stack<T>
where
    T: Send,
    T: Linked<Links<T>>,
{
}

unsafe impl<T> Sync for Stack<T>
where
    T: Sync,
    T: Linked<Links<T>>,
{
}

// === impl Links ===

impl<T> Links<T> {
    /// Returns new [`TransferStack`] links.
    #[cfg(not(loom))]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: UnsafeCell::new(None),
            _unpin: PhantomPinned,
        }
    }

    /// Returns new [`TransferStack`] links.
    #[cfg(loom)]
    #[must_use]
    pub fn new() -> Self {
        Self {
            next: UnsafeCell::new(None),
            _unpin: PhantomPinned,
        }
    }
}

/// # Safety
///
/// Types containing [`Links`] may be `Send`: the pointers within the `Links` may
/// mutably alias another value, but the links can only be _accessed_ by the
/// owner of the [`TransferStack`] itself, because the pointers are private. As
/// long as [`TransferStack`] upholds its own invariants, `Links` should not
/// make a type `!Send`.
unsafe impl<T: Send> Send for Links<T> {}

/// # Safety
///
/// Types containing [`Links`] may be `Send`: the pointers within the `Links` may
/// mutably alias another value, but the links can only be _accessed_ by the
/// owner of the [`TransferStack`] itself, because the pointers are private. As
/// long as [`TransferStack`] upholds its own invariants, `Links` should not
/// make a type `!Send`.
unsafe impl<T: Sync> Sync for Links<T> {}

impl<T> fmt::Debug for Links<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("transfer_stack::Links { ... }")
    }
}

impl<T> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod loom {
    use super::*;
    use crate::loom::{
        self,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread,
    };
    use test_util::Entry;

    #[test]
    fn multithreaded_push() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let threads = Arc::new(AtomicUsize::new(2));
            let thread1 = thread::spawn({
                let stack = stack.clone();
                let threads = threads.clone();
                move || {
                    Entry::push_all(&stack, 1, PUSHES);
                    threads.fetch_sub(1, Ordering::Relaxed);
                }
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                let threads = threads.clone();
                move || {
                    Entry::push_all(&stack, 2, PUSHES);
                    threads.fetch_sub(1, Ordering::Relaxed);
                }
            });

            let mut seen = Vec::new();

            loop {
                seen.extend(stack.take_all().map(|entry| entry.val));

                if threads.load(Ordering::Relaxed) == 0 {
                    break;
                }

                thread::yield_now();
            }

            seen.extend(stack.take_all().map(|entry| entry.val));

            seen.sort();
            assert_eq!(seen, vec![10, 11, 20, 21]);

            thread1.join().unwrap();
            thread2.join().unwrap();
        })
    }

    #[test]
    fn multithreaded_pop() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let thread1 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 1, PUSHES)
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 2, PUSHES)
            });

            let thread3 = thread::spawn({
                let stack = stack.clone();
                move || stack.take_all().map(|entry| entry.val).collect::<Vec<_>>()
            });

            let seen_thread0 = stack.take_all().map(|entry| entry.val).collect::<Vec<_>>();
            let seen_thread3 = thread3.join().unwrap();

            thread1.join().unwrap();
            thread2.join().unwrap();

            let seen_thread0_final = stack.take_all().map(|entry| entry.val).collect::<Vec<_>>();

            let mut all = dbg!(seen_thread0);
            all.extend(dbg!(seen_thread3));
            all.extend(dbg!(seen_thread0_final));

            all.sort();
            assert_eq!(all, vec![10, 11, 20, 21]);
        })
    }

    #[test]
    fn doesnt_leak() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let thread1 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 1, PUSHES)
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 2, PUSHES)
            });

            tracing::info!("dropping stack");
            drop(stack);

            thread1.join().unwrap();
            thread2.join().unwrap();
        })
    }

    #[test]
    fn take_all_doesnt_leak() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let thread1 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 1, PUSHES)
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 2, PUSHES)
            });

            thread1.join().unwrap();
            thread2.join().unwrap();

            let take_all = stack.take_all();

            tracing::info!("dropping stack");
            drop(stack);

            tracing::info!("dropping take_all");
            drop(take_all);
        })
    }

    #[test]
    fn take_all_doesnt_leak_racy() {
        const PUSHES: i32 = 2;
        loom::model(|| {
            let stack = Arc::new(TransferStack::new());
            let thread1 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 1, PUSHES)
            });

            let thread2 = thread::spawn({
                let stack = stack.clone();
                move || Entry::push_all(&stack, 2, PUSHES)
            });

            let take_all = stack.take_all();

            thread1.join().unwrap();
            thread2.join().unwrap();

            tracing::info!("dropping stack");
            drop(stack);

            tracing::info!("dropping take_all");
            drop(take_all);
        })
    }

    #[test]
    fn unsync() {
        loom::model(|| {
            let mut stack = Stack::<Entry>::new();
            stack.push(Entry::new(1));
            stack.push(Entry::new(2));
            stack.push(Entry::new(3));
            let mut take_all = stack.take_all();

            for i in (1..=3).rev() {
                assert_eq!(take_all.next().unwrap().val, i);
                stack.push(Entry::new(10 + i));
            }

            let mut i = 11;
            for entry in stack.take_all() {
                assert_eq!(entry.val, i);
                i += 1;
            }
        })
    }

    #[test]
    fn unsync_doesnt_leak() {
        loom::model(|| {
            let mut stack = Stack::<Entry>::new();
            stack.push(Entry::new(1));
            stack.push(Entry::new(2));
            stack.push(Entry::new(3));
        })
    }

    #[test]
    fn transfer_stack_is_empty() {
        loom::model(|| {
            let stack = TransferStack::<Entry>::new();
            assert!(stack.is_empty(), "a new stack is empty");

            stack.push(Entry::new(1));
            assert!(!stack.is_empty(), "a stack with one element is not empty");

            stack.push(Entry::new(2));
            assert!(!stack.is_empty(), "a stack with two elements is not empty");

            let taken = stack.take_all();
            assert!(stack.is_empty(), "the stack is empty after `take_all`");
            assert_eq!(
                taken.into_iter().count(),
                2,
                "`take_all` took both elements"
            );
        })
    }
}

#[cfg(test)]
mod test {
    use super::{test_util::Entry, *};

    #[test]
    fn stack_is_send_sync() {
        crate::util::assert_send_sync::<TransferStack<Entry>>()
    }

    #[test]
    fn links_are_send_sync() {
        crate::util::assert_send_sync::<Links<Entry>>()
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;
    use crate::loom::alloc;
    use core::pin::Pin;
    use core::ptr;

    #[pin_project::pin_project]
    pub(crate) struct Entry {
        #[pin]
        links: Links<Entry>,
        pub(crate) val: i32,
        track: alloc::Track<()>,
    }

    // ----------------------------------------------------------------------
    // Helper impls for `sorted_list`
    impl PartialEq for Entry {
        fn eq(&self, other: &Self) -> bool {
            self.val.eq(&other.val)
        }
    }

    impl Eq for Entry {}

    impl PartialOrd for Entry {
        fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for Entry {
        fn cmp(&self, other: &Self) -> core::cmp::Ordering {
            self.val.cmp(&other.val)
        }
    }
    // ----------------------------------------------------------------------

    unsafe impl Linked<Links<Self>> for Entry {
        type Handle = Pin<Box<Entry>>;

        fn into_ptr(handle: Pin<Box<Entry>>) -> NonNull<Self> {
            unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(handle))) }
        }

        unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
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

        unsafe fn links(target: NonNull<Self>) -> NonNull<Links<Self>> {
            let links = ptr::addr_of_mut!((*target.as_ptr()).links);
            // Safety: it's fine to use `new_unchecked` here; if the pointer that we
            // offset to the `links` field is not null (which it shouldn't be, as we
            // received it as a `NonNull`), the offset pointer should therefore also
            // not be null.
            NonNull::new_unchecked(links)
        }
    }

    impl Entry {
        pub(crate) fn new(val: i32) -> Pin<Box<Entry>> {
            Box::pin(Entry {
                links: Links::new(),
                val,
                track: alloc::Track::new(()),
            })
        }

        pub(super) fn push_all(stack: &TransferStack<Self>, thread: i32, n: i32) {
            for i in 0..n {
                let entry = Self::new((thread * 10) + i);
                stack.push(entry);
            }
        }
    }
}
