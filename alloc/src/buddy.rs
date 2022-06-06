use core::{
    alloc::{GlobalAlloc, Layout},
    cmp, mem, ptr,
};
use hal_core::{
    mem::{
        page::{self, AllocErr, PageRange, Size},
        Region, RegionKind,
    },
    Address, PAddr, VAddr,
};
use mycelium_util::fmt;
use mycelium_util::intrusive::{list, Linked, List};
use mycelium_util::math::Log2;
use mycelium_util::sync::{
    atomic::{
        AtomicUsize,
        Ordering::{AcqRel, Acquire, Relaxed},
    },
    spin,
};

#[derive(Debug)]
pub struct Alloc<L = [spin::Mutex<List<Free>>; 32]> {
    /// Minimum allocateable page size in bytes.
    ///
    /// Free blocks on `free_lists[0]` are one page of this size each. For each
    /// index higher in the array of free lists, the blocks on that free list
    /// are 2x as large.
    min_size: usize,

    base_vaddr: AtomicUsize,
    vm_offset: AtomicUsize,

    /// Cache this so we don't have to re-evaluate it.
    min_size_log2: usize,

    /// Total size of the heap.
    heap_size: AtomicUsize,

    /// Array of free lists by "order". The order of an block is the number
    /// of times the minimum page size must be doubled to reach that block's
    /// size.
    free_lists: L,
}

type Result<T> = core::result::Result<T, AllocErr>;

pub struct Free {
    magic: usize,
    links: list::Links<Self>,
    meta: Region,
}

// ==== impl Alloc ===

impl Alloc {
    #[cfg(not(loom))]
    pub const fn new_default(min_size: usize) -> Self {
        Self::new(
            min_size,
            // haha this is cool and fun
            [
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
                spin::Mutex::new(List::new()),
            ],
        )
    }
}

impl<L> Alloc<L> {
    #[cfg(not(loom))]
    pub const fn new(mut min_size: usize, free_lists: L) -> Self {
        // ensure we don't split memory into regions too small to fit the free
        // block header in them.
        let free_block_size = mem::size_of::<Free>();
        if min_size < free_block_size {
            min_size = free_block_size;
        }
        // round the minimum block size up to the next power of two, if it isn't
        // a power of two (`size_of::<Free>` is *probably* 48 bytes on 64-bit
        // architectures...)
        min_size = min_size.next_power_of_two();
        Self {
            min_size,
            base_vaddr: AtomicUsize::new(usize::MAX),
            vm_offset: AtomicUsize::new(0),
            min_size_log2: mycelium_util::math::usize_const_log2_ceil(min_size),
            heap_size: AtomicUsize::new(0),
            free_lists,
        }
    }

    pub fn set_vm_offset(&self, offset: VAddr) {
        self.vm_offset
            .compare_exchange(0, offset.as_usize(), AcqRel, Acquire)
            .expect("dont do this twice lol");
    }

    /// Returns the minimum allocatable size, in bytes.
    pub fn min_size(&self) -> usize {
        self.min_size
    }

    /// Returns the current amount of allocatable memory, in bytes.
    pub fn free_size(&self) -> usize {
        self.heap_size.load(Acquire)
    }

    /// Returns the base virtual memory offset.
    // TODO(eliza): nicer way to configure this?
    fn offset(&self) -> usize {
        let addr = self.vm_offset.load(Relaxed);
        debug_assert_ne!(addr, 0, "you didn't initialize the heap yet dipshit");
        addr
    }

    /// Returns the size of the allocation for a given order
    fn size_for_order(&self, order: usize) -> usize {
        1 << (self.min_size_log2 + order)
    }

    /// Returns the actual size of the block that must be allocated for an
    /// allocation of `len` pages of `page_size`.
    fn size_for(&self, layout: Layout) -> Option<usize> {
        let mut size = layout.size();
        let align = layout.align();
        size = cmp::max(size, align);

        // Round up to the heap's minimum allocateable size.
        if size < self.min_size {
            tracing::warn!(
                size,
                min_size = self.min_size,
                layout.size = layout.size(),
                layout.align = layout.align(),
                "size is less than the minimum page size; rounding up"
            );
            size = self.min_size;
        }

        // Is the size a power of two?
        if !size.is_power_of_two() {
            let next_pow2 = size.next_power_of_two();
            tracing::trace!(
                layout.size = size,
                next_pow2,
                "size is not a power of two, rounding up..."
            );
            size = next_pow2;
        }
        // debug_assert!(
        //     size.is_power_of_two(),
        //     "somebody constructed a bad layout! don't do that!"
        // );

        // Is there enough room to meet this allocation request?
        let available = self.heap_size.load(Acquire);
        if size > available {
            tracing::error!(
                size,
                available,
                layout.size = layout.size(),
                layout.align = layout.align(),
                "out of memory!"
            );
            return None;
        }

        Some(size)
    }

    /// Returns the order of the block that would be allocated for a range of
    /// `len` pages of size `size`.
    fn order_for(&self, layout: Layout) -> Option<usize> {
        self.size_for(layout).map(|size| self.order_for_size(size))
    }

    /// Returns the order of a block of `size` bytes.
    fn order_for_size(&self, size: usize) -> usize {
        size.log2_ceil() - self.min_size_log2
    }
}

impl<L> Alloc<L>
where
    L: AsRef<[spin::Mutex<List<Free>>]>,
{
    pub fn dump_free_lists(&self) {
        for (order, list) in self.free_lists.as_ref().iter().enumerate() {
            let _span =
                tracing::debug_span!("free_list", order, size = self.size_for_order(order),)
                    .entered();
            match list.try_lock() {
                Some(list) => {
                    for entry in list.iter() {
                        tracing::debug!("entry={entry:?}");
                    }
                }
                None => {
                    tracing::debug!("<THIS IS THE ONE WHERE THE PANIC HAPPENED LOL>");
                }
            }
        }
    }

    /// Adds a memory region to the heap from which pages may be allocated.
    #[tracing::instrument(skip(self), level = "debug")]
    pub unsafe fn add_region(&self, region: Region) -> core::result::Result<(), ()> {
        // Is the region in use?
        if region.kind() != RegionKind::FREE {
            tracing::warn!(?region, "cannot add to page allocator, region is not free");
            return Err(());
        }

        let mut next_region = Some(region);
        while let Some(mut region) = next_region.take() {
            let size = region.size();
            let base = region.base_addr();
            let _span = tracing::trace_span!("adding_region", size, ?base).entered();
            tracing::info!(region.size = size, region.base_addr = ?base, "adding region");

            // Is the region aligned on the heap's minimum page size? If not, we
            // need to align it.
            if !base.is_aligned(self.min_size) {
                let new_base = base.align_up(self.min_size);
                tracing::trace!(region.new_base = ?new_base, "base address not aligned!");
                region = Region::new(new_base, region.size(), RegionKind::FREE);
            }

            // Is the size of the region a power of two? The buddy block algorithm
            // requires each free block to be a power of two.
            if !size.is_power_of_two() {
                // If the region is not a power of two, split it down to the nearest
                // power of two.
                let prev_power_of_two = prev_power_of_two(size);
                tracing::debug!(prev_power_of_two, "not a power of two!");
                let region2 = region.split_back(prev_power_of_two).unwrap();

                // If the region we split off is larger than the minimum page size,
                // we can try to add it as well.
                if region2.size() >= self.min_size {
                    tracing::debug!("adding split-off region");
                    next_region = Some(region2);
                } else {
                    // Otherwise, we can't use it --- we'll have to leak it.
                    // TODO(eliza):
                    //  figure out a nice way to use stuff that won't fit for "some
                    //  other purpose"?
                    // NOTE:
                    //  in practice these might just be the two "bonus bytes" that
                    //  the free regions in our memory map have for some kind of
                    //  reason (on x84).
                    // TODO(eliza):
                    //  figure out why free regions in the memory map are all
                    //  misaligned by two bytes.
                    tracing::debug!(
                        region = ?region2,
                        min_size = self.min_size,
                        "leaking a region smaller than min page size"
                    );
                }
            }

            // Update the base virtual address of the heap.
            let region_vaddr = region.base_addr().as_usize() + self.offset();
            self.base_vaddr.fetch_min(region_vaddr, AcqRel);

            // ...and actually add the block to a free list.
            let block = Free::new(region, self.offset());
            unsafe { self.push_block(block) };
        }

        Ok(())
    }

    unsafe fn alloc_inner(&self, layout: Layout) -> Option<ptr::NonNull<Free>> {
        // This is the minimum order necessary for the requested allocation ---
        // the first free list we'll check.
        let order = self.order_for(layout)?;
        tracing::trace!(?order);

        // Try each free list, starting at the minimum necessary order.
        for (idx, free_list) in self.free_lists.as_ref()[order..].iter().enumerate() {
            tracing::trace!(curr_order = idx + order);

            // Is there an available block on this free list?
            let mut free_list = free_list.lock();
            if let Some(mut block) = free_list.pop_back() {
                let block = unsafe { block.as_mut() };
                tracing::trace!(?block, ?free_list, "found");

                // Unless this is the free list on which we'd expect to find a
                // block of the requested size (the first free list we checked),
                // the block is larger than the requested allocation. In that
                // case, we'll need to split it down and push the remainder onto
                // the appropriate free lists.
                if idx > 0 {
                    let curr_order = idx + order;
                    tracing::trace!(?curr_order, ?order, "split down");
                    self.split_down(block, curr_order, order);
                }

                // Change the block's magic to indicate that it is allocated, so
                // that we can avoid checking the free list if we try to merge
                // it before the first word is written to.
                block.make_busy();
                tracing::trace!(?block, "made busy");
                return Some(block.into());
            }
        }
        None
    }

    unsafe fn dealloc_inner(&self, paddr: PAddr, layout: Layout) -> Result<()> {
        // Find the order of the free list on which the freed range belongs.
        let min_order = self.order_for(layout);
        tracing::trace!(?min_order);
        let min_order = min_order.ok_or_else(AllocErr::oom)?;

        let size = match self.size_for(layout) {
            Some(size) => size,
            // XXX(eliza): is it better to just leak it?
            None => panic!(
                "couldn't determine the correct layout for an allocation \
                we previously allocated successfully, what the actual fuck!\n \
                addr={:?}; layout={:?}; min_order={}",
                paddr, layout, min_order,
            ),
        };

        // Construct a new free block.
        let mut block =
            unsafe { Free::new(Region::new(paddr, size, RegionKind::FREE), self.offset()) };

        // Starting at the minimum order on which the freed range will fit
        for (idx, free_list) in self.free_lists.as_ref()[min_order..].iter().enumerate() {
            let curr_order = idx + min_order;
            let mut free_list = free_list.lock();

            // Is there a free buddy block at this order?
            if let Some(mut buddy) = unsafe { self.take_buddy(block, curr_order, &mut free_list) } {
                // Okay, merge the blocks, and try the next order!
                if buddy < block {
                    mem::swap(&mut block, &mut buddy);
                }
                unsafe {
                    block.as_mut().merge(buddy.as_mut());
                }
                tracing::trace!(?buddy, ?block, "merged with buddy");
                // Keep merging!
            } else {
                // Okay, we can't keep merging, so push the block on the current
                // free list.
                free_list.push_front(block);
                tracing::trace!("deallocated block");
                return Ok(());
            }
        }

        unreachable!("we will always iterate over at least one free list");
    }

    #[tracing::instrument(skip(self), level = "trace")]
    unsafe fn push_block(&self, block: ptr::NonNull<Free>) {
        let block_size = block.as_ref().size();
        let order = self.order_for_size(block_size);
        tracing::trace!(block = ?block.as_ref(), block.order = order);
        let free_lists = self.free_lists.as_ref();
        if order > free_lists.len() {
            todo!("(eliza): choppity chop chop down the block!");
        }
        free_lists[order].lock().push_front(block);
        let mut sz = self.heap_size.load(Acquire);
        while let Err(actual) =
            // TODO(eliza): if this overflows that's bad news lol...
            self.heap_size
                    .compare_exchange_weak(sz, sz + block_size, AcqRel, Acquire)
        {
            sz = actual;
        }
    }

    /// Removes `block`'s buddy from the free list and returns it, if it is free
    ///
    /// The "buddy" of a block is the block from which that block was split off
    /// to reach its current order, and therefore the block with which it could
    /// be merged to reach the target order.
    unsafe fn take_buddy(
        &self,
        block: ptr::NonNull<Free>,
        order: usize,
        free_list: &mut List<Free>,
    ) -> Option<ptr::NonNull<Free>> {
        let size = self.size_for_order(order);
        let base = self.base_vaddr.load(Relaxed);

        if base == usize::MAX {
            // This is a bug.
            tracing::error!("cannot find buddy block; heap not initialized!");
            return None;
        }

        tracing::trace!(
            heap.base = fmt::hex(base),
            block.addr = ?block,
            block.order = order,
            block.size = size,
            "calculating buddy..."
        );

        // Find the relative offset of `block` from the base of the heap.
        let rel_offset = block.as_ptr() as usize - base;
        let buddy_offset = rel_offset ^ size;
        let buddy = (base + buddy_offset) as *mut Free;
        tracing::trace!(
            block.rel_offset = fmt::hex(rel_offset),
            buddy.offset = fmt::hex(buddy_offset),
            buddy.addr = ?buddy,
        );

        if ptr::eq(buddy as *const _, block.as_ptr() as *const _) {
            tracing::trace!("buddy block is the same as self");
            return None;
        }

        let buddy = unsafe {
            // Safety: we constructed this address via a usize add of two
            // values we know are not 0, so this should not be null, and
            // it's okay to use `new_unchecked`.
            // TODO(eliza): we should probably die horribly if that add
            // *does* overflow i guess...
            ptr::NonNull::new_unchecked(buddy)
        };

        // Check if the buddy block is definitely in use before removing it from
        // the free list.
        //
        // `is_maybe_free` returns a *hint* --- if it returns `false`, we know
        // the block is in use, so we don't have to remove it from the free list.
        if unsafe { buddy.as_ref().is_maybe_free() } {
            // Okay, now try to remove the buddy from its free list. If it's not
            // free, this will return `None`.
            return free_list.remove(buddy);
        }

        // Otherwise, the buddy block is currently in use.
        None
    }

    /// Split a block of order `order` down to order `target_order`.
    #[tracing::instrument(skip(self), level = "trace")]
    fn split_down(&self, block: &mut Free, mut order: usize, target_order: usize) {
        let mut size = block.size();
        debug_assert_eq!(
            size,
            self.size_for_order(order),
            "a block was a weird size for some reason"
        );

        let free_lists = self.free_lists.as_ref();
        while order > target_order {
            order -= 1;
            size >>= 1;

            tracing::trace!(order, target_order, size, ?block, "split at");
            let new_block = block
                .split_back(size, self.offset())
                .expect("block too small to split!");
            tracing::trace!(?block, ?new_block);
            free_lists[order].lock().push_front(new_block);
        }
    }
}

unsafe impl<S, L> page::Alloc<S> for Alloc<L>
where
    L: AsRef<[spin::Mutex<List<Free>>]>,
    S: Size + fmt::Display,
{
    /// Allocate a range of at least `len` pages.
    ///
    /// If `len` is not a power of two, the length is rounded up to the next
    /// power of two. The returned `PageRange` struct stores the actual length
    /// of the allocated page range.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&self, size: S, len: usize) -> Result<PageRange<PAddr, S>> {
        let span = tracing::trace_span!("alloc_range", size = size.as_usize(), len);
        let _e = span.enter();

        debug_assert!(
            size.as_usize().is_power_of_two(),
            "page size must be a power of 2; size={}",
            size
        );

        let actual_len = if len.is_power_of_two() {
            len
        } else {
            let next = len.next_power_of_two();
            tracing::debug!(
                requested.len = len,
                rounded.len = next,
                "rounding up page range length to next power of 2"
            );
            next
        };

        let total_size = size
            .as_usize()
            // If the size of the page range would overflow, we *definitely*
            // can't allocate that lol.
            .checked_mul(actual_len)
            .ok_or_else(AllocErr::oom)?;

        debug_assert!(
            total_size.is_power_of_two(),
            "total size of page range must be a power of 2; total_size={} size={} len={}",
            total_size,
            size,
            actual_len
        );

        #[cfg(debug_assertions)]
        let layout = Layout::from_size_align(total_size, size.as_usize())
            .expect("page ranges should have valid (power of 2) size/align");
        #[cfg(not(debug_assertions))]
        let layout = unsafe {
            // Safety: we expect all page sizes to be powers of 2.
            Layout::from_size_align_unchecked(total_size, size.as_usize())
        };

        // Try to allocate the page range
        let block = unsafe { self.alloc_inner(layout) }.ok_or_else(AllocErr::oom)?;

        // Return the allocation!
        let range = unsafe { block.as_ref() }.region().page_range(size);
        tracing::debug!(
            ?range,
            requested.size = size.as_usize(),
            requested.len = len,
            actual.len = actual_len,
            "allocated"
        );
        range.map_err(Into::into)
    }

    /// Deallocate a range of pages.
    ///
    /// # Returns
    /// - `Ok(())` if a range of pages was successfully deallocated
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc_range(&self, range: PageRange<PAddr, S>) -> Result<()> {
        let page_size = range.page_size().as_usize();
        let len = range.len();
        let base = range.base_addr();
        let span = tracing::trace_span!(
            "dealloc_range",
            range.base = ?base,
            range.page_size = page_size,
            range.len = len
        );
        let _e = span.enter();

        let total_size = page_size
            .checked_mul(len)
            .expect("page range size shouldn't overflow, this is super bad news");

        #[cfg(debug_assertions)]
        let layout = Layout::from_size_align(total_size, page_size)
            .expect("page ranges should be well-aligned");
        #[cfg(not(debug_assertions))]
        let layout = unsafe {
            // Safety: we expect page ranges to be well-aligned.
            Layout::from_size_align_unchecked(total_size, page_size)
        };

        unsafe {
            self.dealloc_inner(base, layout)?;
        }

        tracing::debug!(
            range.base = ?range.base_addr(),
            range.page_size = range.page_size().as_usize(),
            range.len = range.len(),
            "deallocated"
        );
        Ok(())
    }
}

unsafe impl GlobalAlloc for Alloc {
    #[tracing::instrument(level = "trace", skip(self))]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_inner(layout)
            .map(ptr::NonNull::as_ptr)
            .unwrap_or_else(ptr::null_mut)
            .cast::<u8>()
    }

    #[tracing::instrument(level = "trace", skip(self))]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let addr = match (ptr as usize).checked_sub(self.offset()) {
            Some(addr) => addr,
            None => panic!(
                "pointer is not to a kernel VAddr! ptr={:p}; offset={:x}",
                ptr,
                self.offset()
            ),
        };

        let addr = PAddr::from_usize(addr);
        match self.dealloc_inner(addr, layout) {
            Ok(_) => {}
            Err(_) => panic!(
                "deallocating {:?} with layout {:?} failed! this shouldn't happen!",
                addr, layout
            ),
        }
    }
}

// ==== impl Free ====

impl Free {
    const MAGIC: usize = 0xF4EE_B10C; // haha lol it spells "free block"
    const MAGIC_BUSY: usize = 0xB4D_B10C;

    /// # Safety
    ///
    /// Don't construct a free list entry for a region that isn't actually free,
    /// that would be, uh, bad, lol.
    pub unsafe fn new(region: Region, offset: usize) -> ptr::NonNull<Free> {
        tracing::trace!(?region, offset = fmt::hex(offset));

        let ptr = ((region.base_addr().as_ptr::<Free>() as usize) + offset) as *mut _;
        let nn = ptr::NonNull::new(ptr)
            .expect("definitely don't try to free the zero page; that's evil");

        ptr::write_volatile(
            ptr,
            Free {
                magic: Self::MAGIC,
                links: list::Links::default(),
                meta: region,
            },
        );
        nn
    }

    pub fn split_front(&mut self, size: usize, offset: usize) -> Option<ptr::NonNull<Self>> {
        debug_assert_eq!(
            self.magic,
            Self::MAGIC,
            "MY MAGIC WAS MESSED UP! self={:#?}, self.magic={:#x}",
            self,
            self.magic
        );
        let new_meta = self.meta.split_front(size)?;
        let new_free = unsafe { Self::new(new_meta, offset) };
        Some(new_free)
    }

    pub fn split_back(&mut self, size: usize, offset: usize) -> Option<ptr::NonNull<Self>> {
        debug_assert_eq!(
            self.magic,
            Self::MAGIC,
            "MY MAGIC WAS MESSED UP! self={:#?}, self.magic={:#x}",
            self,
            self.magic
        );

        let new_meta = self.meta.split_back(size)?;
        debug_assert_ne!(new_meta, self.meta);
        tracing::trace!(?new_meta, ?self.meta, "split meta");

        let new_free = unsafe { Self::new(new_meta, offset) };
        debug_assert_ne!(new_free, ptr::NonNull::from(self));

        Some(new_free)
    }

    pub fn merge(&mut self, other: &mut Self) {
        debug_assert_eq!(
            self.magic,
            Self::MAGIC,
            "MY MAGIC WAS MESSED UP! self={:#?}, self.magic={:#x}",
            self,
            self.magic
        );
        debug_assert_eq!(
            self.magic,
            Self::MAGIC,
            "THEIR MAGIC WAS MESSED UP! self={:#?}, self.magic={:#x}",
            self,
            self.magic
        );
        assert!(!other.links.is_linked());
        self.meta.merge(&mut other.meta)
    }

    pub fn region(&self) -> Region {
        self.meta.clone() // XXX(eliza): `Region` should probly be `Copy`.
    }

    pub fn size(&self) -> usize {
        self.meta.size()
    }

    /// Returns `true` if the region *might* be free.
    ///
    /// If this returns false, the region is *definitely* not free. If it
    /// returns true, the region is *possibly* free, and the free list should be
    /// checked.
    ///
    /// This is intended as a hint to avoid traversing the free list for blocks
    /// which are definitely in use, not as an authoritative source.
    #[inline]
    pub fn is_maybe_free(&self) -> bool {
        self.magic == Self::MAGIC
    }

    pub fn make_busy(&mut self) {
        self.magic = Self::MAGIC_BUSY;
    }
}

unsafe impl Linked<list::Links<Self>> for Free {
    type Handle = ptr::NonNull<Free>;

    #[inline]
    fn into_ptr(r: Self::Handle) -> ptr::NonNull<Self> {
        r
    }

    #[inline]
    unsafe fn from_ptr(ptr: ptr::NonNull<Self>) -> Self::Handle {
        ptr
    }

    #[inline]
    unsafe fn links(ptr: ptr::NonNull<Self>) -> ptr::NonNull<list::Links<Self>> {
        ptr::NonNull::from(&ptr.as_ref().links)
    }
}

impl fmt::Debug for Free {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Free")
            .field("magic", &fmt::hex(&self.magic))
            .field("links", &self.links)
            .field("meta", &self.meta)
            .finish()
    }
}

fn prev_power_of_two(n: usize) -> usize {
    (n / 2).next_power_of_two()
}
