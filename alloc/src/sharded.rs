use mycelium_util::intrusive::{Linked, mpsc_queue::{self, MpscQueue}, list::{self, List}};
use core::{cell::UnsafeCell, ptr::{self, NonNull}, marker::PhantomPinned};

const MAX_CORES: usize = 512;

pub struct Heap<L> {
    shards: [Shared; MAX_CORES],
    local: L,
}

struct Shared {

}

#[derive(Debug)]
struct LocalHeap {
    direct_pages: [NonNull<PageHeader>; DIRECT_PAGES],
}

const SMALL_MAX_SIZE: usize = 1024;
const SMALL_BUCKET: usize = 8;
const SMALL_BUCKET_SHIFT: usize = 3;
const DIRECT_PAGES: usize = SMALL_MAX_SIZE / SMALL_BUCKET;

#[derive(Debug)]
struct PageHeader {
    // magic: usize,

    /// Links for the linked list of pages.
    links: list::Links<PageHeader>,

    /// The head of the shared free list for this page.
    shared_free_list: MpscQueue<BlockHeader>,

    /// The head of the local free list for this page.
    local_free_list: List<BlockHeader>,

    /// Number of used blocks on this page
    used_blocks: usize,

    _pin: PhantomPinned,
}


#[derive(Debug)]
struct BlockHeader {
    /// Link blocks to the shared free list (an atomic MPSC queue).
    shared_links: mpsc_queue::Links<BlockHeader>,

    /// Link blocks to the local (owning core's) free list (a non-atomic linked
    /// list).
    ///
    /// # Safety
    ///
    /// This can only be accessed from the CPU core that owns this page.
    // TODO(eliza): add a singly-linked list to `cordyceps`? this doesn't
    // actually need to be doubly linked...
    // TODO(eliza): use a `loom` unsafe cell here?
    local_links: UnsafeCell<list::Links<BlockHeader>>,

    _pin: PhantomPinned,
}

// === impl LocalHeap ===

impl LocalHeap {
    #[inline]
    unsafe fn small_alloc(&mut self, size: usize) -> NonNull<BlockHeader> {
        let bucket = (size + SMALL_BUCKET - 1) >> SMALL_BUCKET_SHIFT;
        let pg = self.direct_pages[bucket].as_mut();

        if let Some(block) = pg.local_free_list.pop_front() {
            pg.used_blocks += 1;
            return block;
        }

        self.alloc_generic(size)
    }

    unsafe fn alloc_generic(&mut self, _size: usize) -> NonNull<BlockHeader> {
        // shared free list
        // or make a new pg from a larger sz class
        todo!("eliza")
    }
}

// === impl PageHeader ===


unsafe impl Linked<list::Links<PageHeader>> for PageHeader {
    type Handle = ptr::NonNull<Self>;

    #[inline]
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> { handle }

    #[inline]
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }

    #[inline]
    unsafe fn links(target: NonNull<Self>) -> NonNull<list::Links<Self>> {
        let target = target.as_ptr();

        // Using the `ptr::addr_of_mut!` macro, we can offset a raw pointer to a
        // raw pointer to a field *without* creating a temporary reference.
        let links = ptr::addr_of_mut!((*target).links);

        // `NonNull::new_unchecked` is safe to use here, because the pointer that
        // we offset was not null, implying that the pointer produced by offsetting
        // it will also not be null.
        NonNull::new_unchecked(links)
    }
}

// === impl BlockHeader ===

unsafe impl Linked<mpsc_queue::Links<BlockHeader>> for BlockHeader {
    type Handle = ptr::NonNull<Self>;

    #[inline]
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> { handle }

    #[inline]
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }

    #[inline]
    unsafe fn links(target: NonNull<Self>) -> NonNull<mpsc_queue::Links<Self>> {
        let target = target.as_ptr();

        // Using the `ptr::addr_of_mut!` macro, we can offset a raw pointer to a
        // raw pointer to a field *without* creating a temporary reference.
        let links = ptr::addr_of_mut!((*target).shared_links);

        // `NonNull::new_unchecked` is safe to use here, because the pointer that
        // we offset was not null, implying that the pointer produced by offsetting
        // it will also not be null.
        NonNull::new_unchecked(links)
    }
}

unsafe impl Linked<list::Links<BlockHeader>> for BlockHeader {
    type Handle = ptr::NonNull<Self>;

    #[inline]
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> { handle }

    #[inline]
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }

    #[inline]
    unsafe fn links(target: NonNull<Self>) -> NonNull<list::Links<Self>> {
        let target = target.as_ptr();

        // Using the `ptr::addr_of_mut!` macro, we can offset a raw pointer to a
        // raw pointer to a field *without* creating a temporary reference.
        let links = (*ptr::addr_of_mut!((*target).local_links)).get();

        // `NonNull::new_unchecked` is safe to use here, because the pointer that
        // we offset was not null, implying that the pointer produced by offsetting
        // it will also not be null.
        NonNull::new_unchecked(links)
    }
}