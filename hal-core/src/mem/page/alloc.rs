use super::{Alloc, AllocErr, PageRange, Size};
use crate::{
    mem::{Region, RegionKind},
    PAddr,
};

#[derive(Debug)]
pub struct MemMapAlloc<M> {
    map: M,
    curr: Option<Region>,
}

impl<M> MemMapAlloc<M>
where
    M: Iterator<Item = Region>,
{
    fn next_region(&mut self) -> Result<Region, AllocErr> {
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
                    // number of pages, or it is not aligned for this size of page.
                    // XXX(eliza): We are about to throw it away! It needs to go
                    // on a free list eventually.
                }
            }
            self.curr = Some(self.next_region()?);
        }
    }

    fn dealloc_range(&self, range: PageRange<PAddr, S>) -> Result<(), AllocErr> {
        // XXX(eliza): free list!
        Ok(()) // bump pointer-y
    }
}
