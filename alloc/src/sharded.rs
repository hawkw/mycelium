use mycelium_util::{
    intrusive::{
        Linked,
        list::{self, List},
        stack::{self, Stack, TransferStack},
}, sync::{atomic::AtomicUsize, CachePadded, cell::UnsafeCell}};
use core::{ptr::{self, NonNull}, marker::PhantomPinned, mem};

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
    page_lists: [List<PageHeader>; DIRECT_PAGES],
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

    shared: CachePadded<PageShared>,
    local: UnsafeCell<PageLocal>,

    _pin: PhantomPinned,
}

#[derive(Debug)]
struct PageShared {
    /// The head of the shared free list for this page.
    free_list: TransferStack<BlockHeader>,
    freed: AtomicUsize,
}

#[derive(Debug)]
struct PageLocal {
    /// The head of the local free list for this page.
    free_list: Stack<BlockHeader>,

    /// Number of used blocks on this page
    used_blocks: usize,
}

#[derive(Debug)]
struct BlockHeader {
    /// Free list links.
    links: stack::Links<BlockHeader>,

    _pin: PhantomPinned,
}

// === impl LocalHeap ===

impl LocalHeap {
    #[inline]
    unsafe fn alloc(&mut self, size: usize) -> NonNull<BlockHeader> {
        let bucket = (size + SMALL_BUCKET - 1) >> SMALL_BUCKET_SHIFT;
        let page = self.direct_pages[bucket].as_mut();
        page.local.with_mut(|local| unsafe { (*local).malloc_in_page() })
            .unwrap_or_else(|| self.alloc_slow(size))
    }

    unsafe fn alloc_slow(&mut self, size: usize) -> NonNull<BlockHeader> {
        let pages = self.page_lists[size_class(size)].iter();
        for page in pages {
            page.collect_free();

            // TODO(eliza): if the page is empty, free it back to the page allocator

            // if the page now has free blocks, allocate one.
            if let Some(block) = page.local.with_mut(|local| unsafe { (*local).malloc_in_page() }) {
                return block;
            }
        }

        todo!("try to alloc a new page from the page allocator")
    }
}

impl PageHeader {
    fn collect_free(&self) {
        self.local.with_mut(|local| unsafe {
            debug_assert!((*local).free_list.is_empty());
            (*local).free_list = self.shared.free_list.take_all();
        })

    }
}

impl PageLocal {
    #[inline(always)]
    fn malloc_in_page(&mut self) -> Option<NonNull<BlockHeader>> {
        let block = self.free_list.pop()?;
        self.used_blocks += 1;
        Some(block)
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

unsafe impl Linked<stack::Links<BlockHeader>> for BlockHeader {
    type Handle = ptr::NonNull<Self>;

    #[inline]
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> { handle }

    #[inline]
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle { ptr }

    #[inline]
    unsafe fn links(target: NonNull<Self>) -> NonNull<stack::Links<Self>> {
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

fn size_class(size: usize) -> usize {
    debug_assert!(size > 0);

    let wsize = align_up(size, mem::size_of::<usize>()) / mem::size_of::<usize>();

    if wsize == 1 {
        return 1;
    }

    if wsize <= 8 {
        return align_up(wsize, 2);
    }

    let wsize = wsize - 1;
    let bit_pos = msb_index(wsize);

    (((bit_pos as usize) << 2)
        | ((wsize >> (bit_pos - 2)) & 3)) - 3
}

const fn align_up(n: usize, multiple: usize) -> usize {
    (n + (multiple - 1)) & !(multiple - 1)
}

const fn msb_index(u: usize) -> u32 {
    usize::BITS - u.leading_zeros()
}