use mycelium_util::{
    intrusive::{
        Linked,
        list::{self, List},
        stack::{self, Stack, TransferStack},
}, sync::{atomic::AtomicUsize, CachePadded, cell::UnsafeCell}};
use core::{ptr::{self, NonNull}, marker, mem, alloc};
use hal_core::{mem::page, CoreLocal};

const MAX_CORES: usize = 512;

pub struct Heap<L, P, S> {
    shards: [LocalHeap<P, S>; MAX_CORES],
    current: L,
}

#[derive(Debug)]
struct LocalHeap<P, S> {
    direct_pages: [NonNull<PageHeader<S>>; DIRECT_PAGES],
    page_lists: [List<PageHeader<S>>; DIRECT_PAGES],
    page_allocator: P,
}

#[derive(Debug)]
#[pin_project::pin_project]
struct PageHeader<S> {
    // magic: usize,

    /// Links for the linked list of pages.
    #[pin]
    links: list::Links<Self>,

    shared: CachePadded<PageShared>,
    local: UnsafeCell<PageLocal>,

    _pin: marker::PhantomPinned,

    _size: marker::PhantomData<fn(S)>,
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

    _pin: marker::PhantomPinned,
}


const SMALL_MAX_SIZE: usize = 1024;
const SMALL_BUCKET: usize = 8;
const SMALL_BUCKET_SHIFT: usize = 3;
const DIRECT_PAGES: usize = SMALL_MAX_SIZE / SMALL_BUCKET;

// === impl Heap ===
impl<L, P, S> Heap<L, P, S>
where
    P: page::Alloc<S>,
    S: page::StaticSize,
    L: CoreLocal<usize>,
{
    pub fn allocate(&self, layout: alloc::Layout) -> Option<NonNull<()>> {
        // TODO(eliza): handle alignment...
        let size = layout.size();
        let core = self.current.with(|core| *core);
        let shard = self.shards.get(core)?;
        unsafe {
            Some(shard.alloc_small(size).cast())
        }
    }

    pub unsafe fn deallocate(&self, ptr: NonNull<()>, layout: alloc::Layout) {
        todo!("eliza: deallocate({ptr:?}, {layout:?})")
    }
}

// === impl LocalHeap ===

impl<P, S> LocalHeap<P, S>
where
    P: page::Alloc<S>,
    S: page::StaticSize,
{
    #[inline]
    unsafe fn alloc_small(&self, size: usize) -> NonNull<BlockHeader> {
        let bucket = (size + SMALL_BUCKET - 1) >> SMALL_BUCKET_SHIFT;
        let page = self.direct_pages[bucket].as_ref();
        page.local.with_mut(|local| unsafe { (*local).malloc_in_page() })
            .unwrap_or_else(|| self.alloc_generic(size))
    }

    #[inline(never)]
    unsafe fn alloc_generic(&self, size: usize) -> NonNull<BlockHeader> {
        let mut cursor = self.page_lists[size_class(size)].cursor_front_mut();
        // advance the cursor to the head of the list.
        cursor.move_next();
        // loop until the cursor has reached the end of the list
        while let Some(page) = cursor.current_mut() {
            let page = page.project();
            let block = page.local.with_mut(|local| unsafe {
                // safety: we are on the thread that owns this page.
                let local = &mut *local;
                // collect the page's shared free list.
                local.collect_free(page.shared);

                // TODO(eliza): if the page is empty, free it back to the
                // page allocator
                // if page.is_empty() {
                //     cursor.remove_current();
                //     self.page_allocator.free(page.page());
                //     return None;
                // }

                // the page is not empty, try to allocate a block from it.
                local.malloc_in_page()
            });

            match block {
                Some(block) => return block,
                None => cursor.move_next(),
            }
        }

        // no pages have space for a new block, try to allocate a new page.
        todo!("try to alloc a new page from the page allocator")
    }
}

impl PageLocal {
    #[inline(always)]
    fn malloc_in_page(&mut self) -> Option<NonNull<BlockHeader>> {
        let block = self.free_list.pop()?;
        self.used_blocks += 1;
        Some(block)
    }

    fn collect_free(&mut self, shared: &PageShared) {
        debug_assert!(self.free_list.is_empty());
        self.free_list = shared.free_list.take_all();
    }
}

// === impl PageHeader ===


unsafe impl<S> Linked<list::Links<Self>> for PageHeader<S> {
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