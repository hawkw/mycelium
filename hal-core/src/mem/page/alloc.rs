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

#[derive(Debug)]
pub struct BuddyAlloc<L = [spin::Mutex<List<Free>>; 32]> {
    /// Minimum allocateable size in bytes.
    pub min_size: usize,

    base_paddr: AtomicUsize,
    vm_offset: AtomicUsize,

    /// Cache this so we don't have to re-evaluate it.
    min_size_log2: usize,

    heap_size: AtomicUsize,

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
            base_paddr: AtomicUsize::new(core::usize::MAX),
            vm_offset: AtomicUsize::new(0),
            min_size_log2: mycelium_util::math::usize_const_log2(min_size),
            heap_size: AtomicUsize::new(0),
            free_lists,
        }
    }

    pub fn set_vm_offset(&self, offset: VAddr) {
        self.vm_offset
            .compare_exchange(0, offset.as_usize(), AcqRel, Acquire)
            .expect("dont do this twice lol");
    }

    fn offset(&self) -> usize {
        let addr = self.vm_offset.load(Relaxed);
        debug_assert_ne!(addr, 0, "you didn't initialize the heap yet dipshit");
        addr
    }

    fn size_for_order(&self, order: usize) -> usize {
        1 << (self.min_size_log2 as usize + order)
    }

    fn size_for<S: Size>(&self, size: S, len: usize) -> Result<usize> {
        let size = size.as_usize() * len;

        // Round up to the heap's minimum allocateable size.
        let size = usize::max(size, self.min_size);

        // let size = size.next_power_of_two();

        let available = self.heap_size.load(Acquire);

        if size > available {
            tracing::error!(size, available, "out of memory!");
            return Err(AllocErr::oom());
        }

        Ok(size)
    }

    fn order_for<S: Size>(&self, size: S, len: usize) -> Result<usize> {
        self.size_for(size, len)
            .map(|size| self.order_for_size(size))
    }

    fn order_for_size(&self, size: usize) -> usize {
        size.log2() - self.min_size_log2
    }
}

impl<L> BuddyAlloc<L>
where
    L: AsRef<[spin::Mutex<List<Free>>]>,
{
    #[tracing::instrument(skip(self), level = "trace")]
    pub unsafe fn add_region(&self, mut region: Region) -> core::result::Result<(), ()> {
        if region.kind() == RegionKind::FREE {
            let size = region.size();
            let base = region.base_addr();
            tracing::info!(region.size = size, region.base_addr = ?base, "add region");

            if !region.base_addr().is_aligned(self.min_size) {
                let new_base = base.align_up(self.min_size);
                tracing::trace!(region.new_base = ?new_base, "base address not aligned!");
                region = Region::new(new_base, region.size(), RegionKind::FREE);
            }

            if !size.is_power_of_two() {
                let prev_power_of_two = prev_power_of_two(size);
                let rem = size - prev_power_of_two;
                tracing::debug!(prev_power_of_two, rem, "not a power of two!");
                let region2 = region.split_back(prev_power_of_two).unwrap();
                if region2.size() >= self.min_size {
                    tracing::debug!("add split part");
                    self.add_region(region2)?;
                } else {
                    tracing::info!(region = ?region2, "leaking a region smaller than min page size");
                }
            }

            self.base_paddr
                .fetch_min(region.base_addr().as_usize(), AcqRel);
            let mut block = Free::new(region, self.offset());
            // self.split_down(block.as_mut(), self.order_for_size(sz), 0);
            unsafe { self.push_block(block) };
            return Ok(());
        }
        Err(())
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

    /// Returns `block`'s buddy, if it is free
    unsafe fn buddy_for(
        &self,
        block: ptr::NonNull<Free>,
        order: usize,
    ) -> Option<ptr::NonNull<Free>> {
        let size = 1 << (self.min_size_log2 + order);
        let base = self.base_paddr.load(Relaxed);
        if base == 0 {
            tracing::warn!("cannot find buddy block; heap not initialized!");
            return None;
        }
        let vm_offset = self.offset();
        tracing::trace!(
            "buddy for base={:x}; block={:p}; vm_offset={:x}",
            base,
            block,
            vm_offset
        );
        let block_paddr = block.as_ptr() as usize - vm_offset;
        let rel_offset = block_paddr - base;
        let buddy_offset = rel_offset ^ size;
        tracing::trace!("buddy_offset={:x}", buddy_offset);
        let buddy = (base + buddy_offset + vm_offset) as *mut Free;
        tracing::trace!("buddy_addr = {:p}", buddy);
        if core::ptr::eq(buddy as *const _, block.as_ptr() as *const _) {
            tracing::trace!("buddy block is the same as self");
            return None;
        }
        if unsafe { (*buddy).magic == Free::MAGIC } {
            return Some(ptr::NonNull::new_unchecked(buddy));
        }
        None
    }

    #[tracing::instrument(skip(self), level = "trace")]
    fn split_down(&self, block: &mut Free, mut order: usize, target_order: usize) {
        let mut size = block.size();
        debug_assert_eq!(size, self.size_for_order(order));
        let free_lists = self.free_lists.as_ref();
        while order > target_order {
            order -= 1;
            size >>= 1;
            tracing::trace!(order, target_order, size, "split at");
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

        let order = self.order_for(size, len)?;
        tracing::trace!(?order);
        for (idx, free_list) in self.free_lists.as_ref()[order..].iter().enumerate() {
            tracing::trace!(curr_order = idx + order);
            // Is there an available block on this free list?
            let mut free_list = free_list.lock();
            if let Some(mut block) = free_list.pop_back() {
                // If the block is larger than the desired size, split it.
                let block = unsafe { block.as_mut() };
                tracing::trace!(?block, ?free_list, "found");
                if idx > 0 {
                    let curr_order = idx + order;
                    tracing::trace!(?curr_order, ?order, "split down");
                    self.split_down(block, curr_order, order);
                }
                // Change the block's magic to indicate that it is allocated.
                // /!\ EXTREMELY SERIOUS WARNING /!\
                // This is ACTUALLY LOAD BEARING. If a block is allocated and
                // the first word isn't immediately written to, it *might* still
                // appear to be "free" when its buddy is deallocated, and the
                // buddy may want to merge with it. When the buddy tries to
                // remove the block from the free list, though, it will be
                // surprised to find that the block is not, in fact, free!
                //
                // Therefore, we need to clobber the block magic, even if it
                // will *probably* be immediately overwritten by whoever
                // allocated the block --- they *might* not overwrite it, or
                // someone else might free the buddy of this block before the
                // user of this block writes to it!
                block.make_busy();
                let range = block.region().page_range(size);
                tracing::trace!(?range);
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
        let min_order = self.order_for(range.page_size(), range.len());
        tracing::trace!(min_order = ?min_order);
        let min_order = min_order?;
        let mut block = unsafe {
            Free::new(
                Region::from_page_range(range, RegionKind::FREE),
                self.offset(),
            )
        };

        for (idx, free_list) in self.free_lists.as_ref()[min_order..].iter().enumerate() {
            let curr_order = idx + min_order;
            let mut free_list = free_list.lock();
            tracing::trace!(curr_order, ?free_list, "check for buddy");
            if let Some(buddy) = unsafe { self.buddy_for(block, curr_order) } {
                tracing::trace!(buddy = ?unsafe { buddy.as_ref() }, "found");
                unsafe {
                    let mut buddy = free_list.remove(buddy).unwrap();
                    block.as_mut().merge(buddy.as_mut());
                }
                tracing::trace!("merged with buddy");
                // Keep merging!
                continue;
            }

            free_list.push_front(block);
            tracing::trace!("done");
            return Ok(());
        }

        Ok(())
    }
}

// ==== impl Free ====

impl Free {
    const MAGIC: usize = 0xF4EE_B10C; // haha lol it spells "free block"
    const MAGIC_BUSY: usize = 0xB4D_B10C;

    pub unsafe fn new(region: Region, offset: usize) -> ptr::NonNull<Free> {
        tracing::trace!(?region, offset = ?format_args!("{:x}", offset));
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
        let new_free = unsafe { Self::new(new_meta, offset) };
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

    pub fn make_busy(&mut self) {
        self.magic = Self::MAGIC_BUSY;
    }
}

unsafe impl list::Linked for Free {
    type Handle = ptr::NonNull<Free>;
    fn as_ptr(r: &Self::Handle) -> ptr::NonNull<Self> {
        *r
    }
    unsafe fn as_handle(ptr: ptr::NonNull<Self>) -> Self::Handle {
        ptr
    }
    unsafe fn links(ptr: ptr::NonNull<Self>) -> ptr::NonNull<list::Links<Self>> {
        ptr::NonNull::from(&ptr.as_ref().links)
    }
}

fn prev_power_of_two(n: usize) -> usize {
    (n / 2).next_power_of_two()
}
