pub use alloc::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use hal_core::{
    boot::BootInfo,
    mem::{
        self,
        page::{self, Alloc as PageAlloc},
    },
    PAddr,
};
use mycelium_alloc::buddy;

#[derive(Debug)]
pub struct Allocator {
    allocator: buddy::Alloc<32>,
    allocating: AtomicUsize,
    deallocating: AtomicUsize,
}

#[derive(Debug, Copy, Clone)]
pub struct State {
    pub(crate) allocating: usize,
    pub(crate) deallocating: usize,
    pub(crate) heap_size: usize,
    pub(crate) allocated: usize,
    pub(crate) min_size: usize,
}

impl Allocator {
    pub const fn new() -> Self {
        Self {
            allocator: buddy::Alloc::new(32),
            allocating: AtomicUsize::new(0),
            deallocating: AtomicUsize::new(0),
        }
    }

    pub fn state(&self) -> State {
        State {
            allocating: self.allocating.load(Ordering::Acquire),
            deallocating: self.deallocating.load(Ordering::Acquire),
            heap_size: self.allocator.total_size(),
            allocated: self.allocator.allocated_size(),
            min_size: self.allocator.min_size(),
        }
    }

    pub(crate) fn init(&self, _bootinfo: &impl BootInfo) {
        // XXX(eliza): this sucks
        self.allocator.set_vm_offset(crate::arch::mm::vm_offset());
        tracing::info!("initialized allocator");
    }

    #[inline]
    pub(crate) unsafe fn add_region(&self, region: mem::Region) {
        self.deallocating.fetch_add(1, Ordering::Release);
        tracing::trace!(?region, "adding to page allocator");
        let added = self.allocator.add_region(region).is_ok();
        tracing::trace!(added);
        self.deallocating.fetch_sub(1, Ordering::Release);
    }

    #[inline]
    pub fn dump_free_lists(&self) {
        self.allocator.dump_free_lists();
    }
}

unsafe impl GlobalAlloc for Allocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocating.fetch_add(1, Ordering::Release);
        let ptr = GlobalAlloc::alloc(&self.allocator, layout);
        self.allocating.fetch_sub(1, Ordering::Release);
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.deallocating.fetch_add(1, Ordering::Release);
        GlobalAlloc::dealloc(&self.allocator, ptr, layout);
        self.deallocating.fetch_sub(1, Ordering::Release);
    }
}

unsafe impl<S> PageAlloc<S> for Allocator
where
    buddy::Alloc<32>: PageAlloc<S>,
    S: page::Size,
{
    #[inline]
    fn alloc_range(
        &self,
        size: S,
        len: usize,
    ) -> Result<page::PageRange<PAddr, S>, page::AllocErr> {
        self.allocating.fetch_add(1, Ordering::Release);
        let res = self.allocator.alloc_range(size, len);
        self.allocating.fetch_sub(1, Ordering::Release);
        res
    }

    #[inline]
    fn dealloc_range(&self, range: page::PageRange<PAddr, S>) -> Result<(), page::AllocErr> {
        self.deallocating.fetch_add(1, Ordering::Release);
        let res = self.allocator.dealloc_range(range);
        self.deallocating.fetch_sub(1, Ordering::Release);
        res
    }
}

// === impl State ===

impl State {
    #[inline]
    #[must_use]
    pub fn in_allocator(&self) -> bool {
        self.allocating > 0 || self.deallocating > 0
    }
}

#[cfg(test)]
mod tests {
    use mycotest::*;

    decl_test! {
        fn basic_alloc() -> TestResult {
            // Let's allocate something, for funsies
            use alloc::vec::Vec;
            let mut v = Vec::new();
            tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
            v.push(5u64);
            tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
            v.push(10u64);
            tracing::info!(vec=?v, vec.addr=?v.as_ptr());
            mycotest::assert_eq!(v.pop(), Some(10));
            mycotest::assert_eq!(v.pop(), Some(5));

            Ok(())
        }
    }

    decl_test! {
        fn alloc_big() {
            use alloc::vec::Vec;
            let mut v = Vec::new();

            for i in 0..2048 {
                v.push(i);
            }

            tracing::info!(vec = ?v);
        }
    }
}
