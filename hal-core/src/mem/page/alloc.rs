use super::{Alloc, AllocErr, PageRange, Size};
use crate::{
    mem::{Region, RegionKind},
    Address, PAddr, VAddr,
};
use core::ptr;
use mycelium_util::intrusive::{list, List};
use mycelium_util::math::Log2;
use mycelium_util::sync::{
    atomic::{
        AtomicUsize,
        Ordering::{AcqRel, Acquire, Relaxed},
    },
    spin,
};
use mycelium_util::trace;

#[derive(Debug)]
pub struct BuddyAlloc<L = [spin::Mutex<List<Free>>; 32]> {
    /// Minimum allocateable page size in bytes.
    ///
    /// Free blocks on free_lists[0] are one page of this size each. For each
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

pub type Result<T> = core::result::Result<T, AllocErr>;
#[derive(Debug)]
pub struct Free {
    magic: usize,
    links: list::Links<Self>,
    meta: Region,
}

// ==== impl BuddyAlloc ===

impl BuddyAlloc {
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

impl<L> BuddyAlloc<L> {
    #[cfg(not(loom))]
    pub const fn new(min_size: usize, free_lists: L) -> Self {
        Self {
            min_size,
            base_vaddr: AtomicUsize::new(core::usize::MAX),
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
    fn size_for<S: Size>(&self, page_size: S, len: usize) -> Result<usize> {
        let mut size = page_size
            .as_usize()
            // If the size of the page range would overflow, we *definitely*
            // can't allocate that lol.
            .checked_mul(len)
            .ok_or_else(AllocErr::oom)?;

        // Round up to the heap's minimum allocateable size.
        if size < self.min_size {
            tracing::warn!(
                size,
                min_size = self.min_size,
                allocation.page_size = page_size.as_usize(),
                allocation.len = len,
                "size is less than the minimum page size; rounding up"
            );
            size = self.min_size;
        }

        // Is the size a power of two?
        if !size.is_power_of_two() {
            let rounded = size.next_power_of_two();
            tracing::warn!(
                size,
                rounded,
                allocation.page_size = page_size.as_usize(),
                allocation.len = len,
                "size is not a power of two; rounding up"
            );
            size = rounded;
        }

        // Is there enough room to meet this allocation request?
        let available = self.heap_size.load(Acquire);
        if size > available {
            tracing::error!(
                size,
                available,
                allocation.page_size = page_size.as_usize(),
                allocation.len = len,
                "out of memory!"
            );
            return Err(AllocErr::oom());
        }

        Ok(size)
    }

    /// Returns the order of the block that would be allocated for a range of
    /// `len` pages of size `size`.
    fn order_for<S: Size>(&self, page_size: S, len: usize) -> Result<usize> {
        self.size_for(page_size, len)
            .map(|size| self.order_for_size(size))
    }

    /// Returns the order of a block of `size` bytes.
    fn order_for_size(&self, size: usize) -> usize {
        size.log2_ceil() - self.min_size_log2
    }
}

impl<L> BuddyAlloc<L>
where
    L: AsRef<[spin::Mutex<List<Free>>]>,
{
    /// Adds a memory region to the heap from which pages may be allocated.
    #[tracing::instrument(skip(self), level = "trace")]
    pub unsafe fn add_region(&self, mut region: Region) -> core::result::Result<(), ()> {
        // Is the region in use?
        if region.kind() != RegionKind::FREE {
            tracing::warn!(?region, "cannot add to page allocator, region is not free");
            return Err(());
        }

        let size = region.size();
        let base = region.base_addr();
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
                self.add_region(region2)?;
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
        Ok(())
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
            self
            .heap_size
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

        if base == core::usize::MAX {
            // This is a bug.
            tracing::error!("cannot find buddy block; heap not initialized!");
            return None;
        }

        tracing::trace!(
            heap.base = trace::hex(base),
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
            block.rel_offset = trace::hex(rel_offset),
            buddy.offset = trace::hex(buddy_offset),
            buddy.addr = ?buddy,
        );

        if core::ptr::eq(buddy as *const _, block.as_ptr() as *const _) {
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
        debug_assert_eq!(size, self.size_for_order(order));

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

unsafe impl<S, L> Alloc<S> for BuddyAlloc<L>
where
    L: AsRef<[spin::Mutex<List<Free>>]>,
    S: Size + core::fmt::Display,
{
    /// Allocate a range of `len` pages.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&self, size: S, len: usize) -> Result<PageRange<PAddr, S>> {
        let span = tracing::trace_span!("alloc_range", size = size.as_usize(), len);
        let _e = span.enter();

        if size.as_usize() > self.min_size {
            // TODO(eliza): huge pages should work!
            tracing::error!(
                requested.size = size.as_usize(),
                requested.len = len,
                "cannot allocate; huge pages are not currently supported!"
            );
            return Err(AllocErr::oom());
        }

        // This is the minimum order necessary for the requested allocation ---
        // the first free list we'll check.
        let order = self.order_for(size, len)?;
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

                // Return the allocation!
                let range = block.region().page_range(size);
                tracing::debug!(
                    ?range,
                    requested.size = size.as_usize(),
                    requested.len = len,
                    "allocated"
                );
                return Ok(range?);
            }
        }
        Err(AllocErr::oom())
    }

    /// Deallocate a range of pages.
    ///
    /// # Returns
    /// - `Ok(())` if a range of pages was successfully deallocated
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc_range(&self, range: PageRange<PAddr, S>) -> Result<()> {
        let span = tracing::trace_span!(
            "dealloc_range",
            range.base = ?range.base_addr(),
            range.page_size = range.page_size().as_usize(),
            range.len = range.len()
        );
        let _e = span.enter();

        // Find the order of the free list on which the freed range belongs.
        let min_order = self.order_for(range.page_size(), range.len());
        tracing::trace!(?min_order);
        let min_order = min_order?;

        // Construct a new free block.
        let mut block = unsafe {
            Free::new(
                Region::from_page_range(range, RegionKind::FREE),
                self.offset(),
            )
        };

        // Starting at the minimum order on which the freed range will fit
        for (idx, free_list) in self.free_lists.as_ref()[min_order..].iter().enumerate() {
            let curr_order = idx + min_order;
            let mut free_list = free_list.lock();

            // Is there a free buddy block at this order?
            if let Some(mut buddy) = unsafe { self.take_buddy(block, curr_order, &mut free_list) } {
                // Okay, merge the blocks, and try the next order!
                unsafe {
                    block.as_mut().merge(buddy.as_mut());
                }
                tracing::trace!("merged with buddy");
                // Keep merging!
            } else {
                // Okay, we can't keep merging, so push the block on the current
                // free list.
                free_list.push_front(block);
                tracing::debug!(
                    range.base = ?range.base_addr(),
                    range.page_size = range.page_size().as_usize(),
                    range.len = range.len(),
                    "deallocated"
                );
                return Ok(());
            }
        }

        unreachable!("we will always iterate over at least one free list");
    }
}

// ==== impl Free ====

impl Free {
    const MAGIC: usize = 0xF4EE_B10C; // haha lol it spells "free block"
    const MAGIC_BUSY: usize = 0xB4D_B10C;

    pub unsafe fn new(region: Region, offset: usize) -> ptr::NonNull<Free> {
        tracing::trace!(?region, offset = trace::hex(offset));

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
        debug_assert_eq!(self.magic, Self::MAGIC);
        let new_meta = self.meta.split_front(size)?;
        let new_free = unsafe { Self::new(new_meta, offset) };
        Some(new_free)
    }

    pub fn split_back(&mut self, size: usize, offset: usize) -> Option<ptr::NonNull<Self>> {
        debug_assert_eq!(self.magic, Self::MAGIC);

        let new_meta = self.meta.split_back(size)?;
        debug_assert_ne!(new_meta, self.meta);

        let new_free = unsafe { Self::new(new_meta, offset) };
        debug_assert_ne!(new_free, ptr::NonNull::from(self));

        Some(new_free)
    }

    pub fn merge(&mut self, other: &mut Self) {
        debug_assert_eq!(self.magic, Self::MAGIC, "self.magic={:x}", self.magic);
        debug_assert_eq!(other.magic, Self::MAGIC, "self.magic={:x}", self.magic);
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

unsafe impl list::Linked for Free {
    type Handle = ptr::NonNull<Free>;
    fn as_ptr(r: &Self::Handle) -> ptr::NonNull<Self> {
        *r
    }
    unsafe fn from_ptr(ptr: ptr::NonNull<Self>) -> Self::Handle {
        ptr
    }
    unsafe fn links(ptr: ptr::NonNull<Self>) -> ptr::NonNull<list::Links<Self>> {
        ptr::NonNull::from(&ptr.as_ref().links)
    }
}

fn prev_power_of_two(n: usize) -> usize {
    (n / 2).next_power_of_two()
}
