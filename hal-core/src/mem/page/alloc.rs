use super::{Alloc, AllocErr, PageRange, Size};
use crate::{
    mem::{Region, RegionKind},
    PAddr,
};
use core::ptr;
use mycelium_util::sync::atomic::{
    AtomicPtr,
    Ordering::{AcqRel, Acquire, Relaxed},
};

#[derive(Debug)]
pub struct MemMapAlloc<M> {
    map: M,
    curr: Option<Region>,
    free: FreeList,
}

#[derive(Debug)]
pub struct BuddyAlloc {
    freelists: [FreeList; 32],
}

#[derive(Debug)]
pub struct FreeList {
    head: AtomicPtr<Free>,
}

#[derive(Debug)]
pub struct Free {
    next: AtomicPtr<Self>,
    meta: Region,
}

impl<M> MemMapAlloc<M>
where
    M: Iterator<Item = Region>,
{
    pub fn new(map: M) -> Self {
        Self {
            map,
            curr: None,
            free: FreeList::new(),
        }
    }

    fn next_region(&mut self) -> Result<Region, AllocErr> {
        unsafe { self.free.pop().map(|ptr| Ok(ptr.as_ref().region())) }
            .unwrap_or_else(|| self.next_map())
    }

    fn next_map(&mut self) -> Result<Region, AllocErr> {
        loop {
            match self.map.next() {
                Some(region) if region.kind() == RegionKind::FREE => return Ok(region),
                // The region was not free. Let's look at the next one.
                Some(_) => continue,
                // The memory map is empty. We cannot allocate any more pages.
                None => return Err(AllocErr::oom()),
            }
        }
    }
}

unsafe impl<S, M> Alloc<S> for MemMapAlloc<M>
where
    S: Size,
    M: Iterator<Item = Region>,
{
    /// Allocate a range of `len` pages.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&mut self, s: S, len: usize) -> Result<PageRange<PAddr, S>, AllocErr> {
        let sz = len * s.size();
        loop {
            if let Some(mut curr) = self.curr.take() {
                if let Some(range) = curr
                    .split_front(sz)
                    .and_then(|range| range.page_range(s).ok())
                {
                    if curr.size() > 0 {
                        self.curr = Some(curr);
                        return Ok(range);
                    }
                } else {
                    // The region is not big enough to contain the requested
                    // number of pages, or it is not aligned for this size of
                    // page.
                    unsafe {
                        self.free.push(Free::new(curr));
                    }
                }
            }
            // TODO(eliza): use the free list...
            self.curr = Some(self.next_region()?);
        }
    }

    fn dealloc_range(&self, range: PageRange<PAddr, S>) -> Result<(), AllocErr> {
        let region = Region::new(range.start().base_address(), range.size(), RegionKind::FREE);
        unsafe {
            self.free.push(Free::new(region));
        }
        Ok(())
    }
}

impl FreeList {
    /// Returns a new empty free list.
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub unsafe fn push(&self, new: ptr::NonNull<Free>) {
        let next = new.as_ptr();
        if let Some(head) = ptr::NonNull::new(self.head.swap(next, AcqRel)) {
            let mut head = unsafe { head.as_ref() };
            while let Err(actual) =
                head.next
                    .compare_exchange(ptr::null_mut(), next, AcqRel, Acquire)
            {
                head = unsafe { &*actual }
            }
        }
    }

    pub fn pop(&self) -> Option<ptr::NonNull<Free>> {
        let mut head_ptr = self.head.load(Relaxed);
        loop {
            let head = ptr::NonNull::new(head_ptr)?;
            let next_ptr = unsafe { head.as_ref() }.next.load(Acquire);
            match self
                .head
                .compare_exchange(head_ptr, next_ptr, AcqRel, Acquire)
            {
                Ok(_) => {
                    assert_eq!(
                        head.as_ptr(),
                        unsafe { head.as_ref() }.meta.base_addr().as_ptr(),
                        "free list corrupted; this is bad and you should feel bad!!!\
                         did you move a node out of the free list?"
                    );
                    return Some(head);
                }
                Err(actual) => head_ptr = actual,
            }
        }
    }
}

impl Free {
    pub unsafe fn new(region: Region) -> ptr::NonNull<Free> {
        let ptr = region.base_addr().as_ptr::<Free>();
        let nn = ptr::NonNull::new(ptr)
            .expect("definitely don't try to free the zero page; that's evil");
        ptr::write_volatile(
            ptr,
            Free {
                next: AtomicPtr::new(ptr::null_mut()),
                meta: region,
            },
        );
        nn
    }

    pub fn region(&self) -> Region {
        self.meta.clone() // XXX(eliza): `Region` should probly be `Copy`.
    }
}
