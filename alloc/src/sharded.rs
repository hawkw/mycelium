#![allow(unstable_name_collisions)]
use sptr::Strict;
use core::{
    alloc, marker, mem,
    ptr::{self, NonNull},
};
use hal_core::{mem::page::{self, Page}, CoreLocal};
use mycelium_util::{
    intrusive::{
        list::{self, List},
        stack::{self, Stack, TransferStack},
        Linked,
    },
    sync::{atomic::{Ordering, AtomicUsize}, cell::UnsafeCell, CachePadded},
};
use crate::{KB, MB};


pub struct Heap<L, P, S> {
    shards: [UnsafeCell<LocalHeap<P, S>>; MAX_CORES],
    current: L,
}

// #[repr(align(4_194_304))] // 4MB aligned
struct SegmentHeader {
    magic: usize,
    /// The ID of the CPU core that owns this segment.
    core_id: usize,

    page_shift: usize,

    pages: SegmentKind,
}

#[derive(Debug)]
enum SegmentKind {
    /// Small pages ([`PageMeta::SMALL_PAGE_SIZE`]).
    Small([PageMeta; SegmentHeader::SMALL_PAGES]),
    Medium([PageMeta; SegmentHeader::MEDIUM_PAGES]),
    Large(PageMeta),
    Huge(PageMeta),
}

#[derive(Debug)]
struct LocalHeap<P, S> {
    /// The ID of the CPU core that this heap corresponds to.
    core_id: usize,
    direct_pages: [NonNull<PageMeta>; DIRECT_PAGES],
    page_lists: [List<PageMeta>; DIRECT_PAGES],
    page_allocator: P,
    _size: marker::PhantomData<fn(S)>,
}

#[derive(Debug)]
#[pin_project::pin_project]
struct PageMeta {
    // magic: usize,
    /// Links for the linked list of pages.
    #[pin]
    links: list::Links<Self>,

    shared: CachePadded<PageShared>,
    local: UnsafeCell<PageLocal>,
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

#[derive(Debug, Default)]
struct BlockHeader {
    /// Free list links.
    links: stack::Links<BlockHeader>,

    _pin: marker::PhantomPinned,
}

// === segment and page sizes ===
// `MI_INTPTR_SHIFT` in mimalloc
const BASE_SHIFT: usize = match mem::size_of::<*const ()>() {
    // assume 128-bit (e.g. CHERI on ARM)
    size if size > mem::size_of::<u64>() => 4,
    size if size == mem::size_of::<u64>() => 3,
    size if size == mem::size_of::<u32>() => 2,
    _ => panic!("pointers must be 32, 64, or 128 bits"),
};

const SMALL_MAX_SIZE: usize = 1024;
const SMALL_BUCKET: usize = 8;
const SMALL_BUCKET_SHIFT: usize = 3;
const DIRECT_PAGES: usize = SMALL_MAX_SIZE / SMALL_BUCKET;
const MAX_CORES: usize = 512;

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

        let ptr = shard.with_mut(|shard| {
            // Safety: we have selected the `LocalHeap` that is owned by this
            // CPU core; it will never be accessed by another core.
            unsafe { (*shard).alloc_small(size) }
        });

        Some(ptr.cast())
    }

    pub unsafe fn deallocate(&self, ptr: NonNull<()>, layout: alloc::Layout) {
        let core = self.current.with(|core| *core);
        let _span = tracing::trace_span!("deallocate", core, ?ptr, ?layout).entered();

        let segment = SegmentHeader::for_addr(ptr).as_mut();
        assert!(segment.magic == SegmentHeader::MAGIC, "segment header had invalid magic");
        let page = segment.page(ptr.as_ptr() as usize);

        let block = {
            let block = ptr.cast::<mem::MaybeUninit<BlockHeader>>().as_mut().write(BlockHeader::default());
            NonNull::from(block)
        };

        if core == segment.core_id {
            // local free
            tracing::trace!("deallocate -> local");
            page.local.with_mut(|local| unsafe {
                let mut local = &mut *local;
                local.free_list.push(block);
                local.used_blocks -= 1;
                if local.used_blocks - page.shared.freed.load(Ordering::Acquire) == 0 {
                    // TODO(eliza): return the page to the page allocator
                }
            });
        } else {
            // remote free
            tracing::trace!("deallocate -> remote");
            page.shared.free_list.push(block);
            page.shared.freed.fetch_add(1, Ordering::Release);
        }

    }
}

// === impl LocalHeap ===

impl<P, S> LocalHeap<P, S>
where
    P: page::Alloc<S>,
    S: page::StaticSize,
{
    #[inline]
    unsafe fn alloc_small(&mut self, size: usize) -> NonNull<BlockHeader> {
        let bucket = (size + SMALL_BUCKET - 1) >> SMALL_BUCKET_SHIFT;
        let page = self.direct_pages[bucket].as_ref();
        page.local
            .with_mut(|local| unsafe { (*local).malloc_in_page() })
            .unwrap_or_else(|| self.alloc_generic(size))
    }

    #[inline(never)]
    unsafe fn alloc_generic(&mut self, size: usize) -> NonNull<BlockHeader> {
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

// === impl SegmentHeader ===

impl SegmentHeader {
    const MAGIC: usize = 0xfacade;
    const ALIGN: usize = 4 * MB;

    const SHIFT: usize = if mem::size_of::<*const ()>() > 4 {
        // 32 MB
        9 + PageMeta::SMALL_SHIFT
    } else {
        // 4MB on 32-bit
        7 + PageMeta::SMALL_SHIFT
    };

    const SIZE: usize = 1 << Self::SHIFT;

    const SMALL_PAGES: usize = Self::SIZE / PageMeta::SMALL_SIZE;
    const MEDIUM_PAGES: usize = Self::SIZE / PageMeta::MEDIUM_SIZE;

    #[inline]
    fn page(&self, addr: usize) -> &PageMeta {
        let idx = addr - self as *const _ as usize;
        self.pages.page(idx)
    }

    fn for_addr(ptr: NonNull<()>) -> NonNull<Self> {
        let ptr = ptr.as_ptr().map_addr(|addr| {
            // find the segment address for the freed pointer
            addr & (!Self::ALIGN)
        }).cast::<Self>();
        NonNull::new(ptr).expect("tried to free a pointer on the zero page, seems bad")
    }
}

// === impl SegmentKind ===

impl SegmentKind {
    // #[inline(always)]
    // const fn page_shift(&self) -> usize {
    //     match self {
    //         Self::Small(_) => PageMeta::SMALL_SHIFT,
    //         Self::Medium(_) => PageMeta::MEDIUM_SHIFT,
    //         Self::Large(_) => todo!("eliza: what is a large page's shift?"),
    //         Self::Huge(_) => todo!("eliza: what is a huge page's shift?"),
    //     }
    // }

    #[inline]
    fn page(&self, idx: usize) -> &PageMeta {
        match self {
            Self::Small(pages) => &pages[idx >> PageMeta::SMALL_SHIFT],
            Self::Medium(pages) => &pages[idx >> PageMeta::MEDIUM_SHIFT],
            Self::Large(page) | Self::Huge(page) => page,
        }
    }
}

// === impl PageHeader ===

impl PageMeta {
    /// Address shift for a small page.
    const SMALL_SHIFT: usize = 13 + BASE_SHIFT; // 64 KB (32 KB on 32-bit)
    /// Size of a small page.
    const SMALL_SIZE: usize = 1 << Self::SMALL_SHIFT;
    /// Maximum size of an object on a small page.
    const SMALL_OBJ_MAX: usize = Self::SMALL_SIZE / 4; // 8 KB on 64-bit
    const SMALL_OBJ_WSIZE_MAX: usize = Self::wsize_max(Self::SMALL_OBJ_MAX);

    /// Address shift for a medium page.
    const MEDIUM_SHIFT: usize = Self::SMALL_SHIFT + 3; // 512 KB on 64-bit
    /// Size of a medium page.
    const MEDIUM_SIZE: usize = 1 << Self::MEDIUM_SHIFT;
    /// Maximum size of an object on a small page.
    const MEDIUM_OBJ_MAX: usize = Self::MEDIUM_SIZE / 4; // 128 KB on 64-bit
    const MEDIUM_OBJ_WSIZE_MAX: usize = Self::wsize_max(Self::MEDIUM_OBJ_MAX);

    const LARGE_OBJ_MAX: usize = SegmentHeader::SIZE / 2; // 32MB on 64-bit
    const LARGE_OBJ_WSIZE_MAX: usize = Self::wsize_max(Self::LARGE_OBJ_MAX);

    /// XXX(eliza): what is a wsize?
    const fn wsize_max(obj_size: usize) -> usize {
        obj_size / mem::size_of::<*const ()>()
    }
}

// #[derive(Copy, Clone, Debug, Eq, PartialEq)]
// struct PageKind {
//     /// Size of a page of this kind.
//     size: usize,
//     /// Number of pages of this size per segment.
//     per_segment: usize,
//     /// Address shift for a page of this size.
//     shift: usize,
//     /// Maximum object size allocatable on a page of this size.
//     obj_max: usize,
//     /// XXX(eliza): what is a wsize?
//     obj_wsize_max: usize,
// }

// impl PageKind {
//     const fn new(shift: usize, obj_max: usize) -> Self {
//         let size = 1 << shift;
//         // XXX(eliza): what is a wsize?
//         let obj_wsize_max = obj_max / mem::size_of::<*const ()>();
//         Self {
//             size,
//             per_segment: SegmentHeader::SIZE / size,
//             shift,
//             obj_max,
//             obj_wsize_max,
//         }
//     }
// }

unsafe impl Linked<list::Links<Self>> for PageMeta {
    type Handle = ptr::NonNull<Self>;

    #[inline]
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> {
        handle
    }

    #[inline]
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

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
    fn into_ptr(handle: Self::Handle) -> NonNull<Self> {
        handle
    }

    #[inline]
    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        ptr
    }

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

// === impl PageSize ===

// impl PageSize {
//     pub fn for_size(size: usize) -> Self {
//         const SMALL_MAX: usize = 8 * KB;
//         const LARGE_MIN: usize = SMALL_MAX + 1;
//         const LARGE_MAX: usize = 512 * KB;
//         match size {
//             0..=SMALL_MAX => Self::Small,
//             LARGE_MIN..=LARGE_MAX => Self::Large,
//             _ => Self::Huge,
//         }
//     }
// }

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

    (((bit_pos as usize) << 2) | ((wsize >> (bit_pos - 2)) & 3)) - 3
}

const fn align_up(n: usize, multiple: usize) -> usize {
    (n + (multiple - 1)) & !(multiple - 1)
}

const fn msb_index(u: usize) -> u32 {
    usize::BITS - u.leading_zeros()
}
