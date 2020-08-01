use super::{Alloc, AllocErr, PageRange, Size};
use crate::{
    mem::{Region, RegionKind},
    PAddr,
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

    base_addr: *const (),

    /// Cache this so we don't have to re-evaluate it.
    min_size_log2: usize,

    heap_size: AtomicUsize,

    free_lists: L,
}

pub type Result<T> = core::result::Result<T, AllocErr>;
#[derive(Debug)]
pub struct Free {
    links: list::Links<Self>,
    meta: Region,
}

// ==== impl BuddyAlloc ===
impl<L> BuddyAlloc<L> {
    fn size_for<S: Size>(&self, size: S, len: usize) -> Result<usize> {
        let size = size.size() * len;

        // Round up to the heap's minimum allocateable size.
        let size = usize::max(size, self.min_size);

        let size = size.next_power_of_two();

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
    unsafe fn push_block(&self, block: ptr::NonNull<Free>) {
        let order = self.order_for_size(block.as_ref().size());
        let free_lists = self.free_lists.as_ref();
        if order > free_lists.len() {
            todo!("(eliza): choppity chop chop down the block!");
        }
        free_lists[order].lock().push_front(block);
    }

    /// Returns `block`'s buddy, if one exists.
    unsafe fn buddy_for(
        &self,
        block: ptr::NonNull<Free>,
        order: usize,
    ) -> Option<ptr::NonNull<Free>> {
        let size = 1 << (self.min_size_log2 + order);
        let rel_offset = self.base_addr as usize - block.as_ptr() as usize;
        let buddy_offset = rel_offset ^ size;
        let buddy = self.base_addr.offset(buddy_offset as isize);
        Some(ptr::NonNull::new_unchecked(buddy as *mut _))
    }

    fn split_down(&self, block: &mut Free, order: usize, target_order: usize) {
        let mut size = block.size();
        let free_lists = self.free_lists.as_ref();
        for order in (order..target_order).rev() {
            size >>= 1;
            let new_block = block.split_front(size).expect("block too small to split!");
            &free_lists[order].lock().push_front(new_block);
        }
    }
}

unsafe impl<S: Size, L> Alloc<S> for BuddyAlloc<L>
where
    L: AsRef<[spin::Mutex<List<Free>>]>,
{
    /// Allocate a range of `len` pages.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&mut self, size: S, len: usize) -> Result<PageRange<PAddr, S>> {
        let order = self.order_for(size, len)?;
        for (curr_order, free_list) in self.free_lists.as_ref()[order..].iter().enumerate() {
            // Is there an available block on this free list?
            if let Some(mut block) = free_list.lock().pop_back() {
                // If the block is larger than the desired size, split it.
                let block = unsafe { block.as_mut() };
                if curr_order > order {
                    self.split_down(block, curr_order, order);
                }
                let range = block.region().page_range(size)?;
                return Ok(range);
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
        let min_order = self.order_for(range.page_size(), range.len())?;

        for (curr_order, free_list) in self.free_lists.as_ref()[min_order..].iter().enumerate() {}
        unimplemented!()
    }
}

// ==== impl Free ====

impl Free {
    pub unsafe fn new(region: Region) -> ptr::NonNull<Free> {
        let ptr = region.base_addr().as_ptr::<Free>();
        let nn = ptr::NonNull::new(ptr)
            .expect("definitely don't try to free the zero page; that's evil");
        ptr::write_volatile(
            ptr,
            Free {
                links: list::Links::default(),
                meta: region,
            },
        );
        nn
    }

    pub fn split_front(&mut self, size: usize) -> Option<ptr::NonNull<Self>> {
        let new_meta = self.meta.split_front(size)?;
        let new_free = unsafe { Self::new(new_meta) };
        Some(new_free)
    }

    pub fn region(&self) -> Region {
        self.meta.clone() // XXX(eliza): `Region` should probly be `Copy`.
    }

    pub fn size(&self) -> usize {
        self.meta.size()
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
