pub use alloc::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use hal_core::{
    boot::BootInfo,
    mem::{
        self,
        page::{self, Alloc as PageAlloc},
    },
    PAddr,
};
use mycelium_alloc::{buddy, bump};
use mycelium_util::{fmt, math::Logarithm};

#[derive(Debug)]
pub struct Allocator {
    bump: bump::Alloc<BUMP_REGION_SIZE>,
    allocator: buddy::Alloc<32>,
    /// If true, only the bump region is active.
    bump_mode: AtomicBool,
    allocating: AtomicUsize,
    deallocating: AtomicUsize,
}

/// 1k is enough for anyone.
const BUMP_REGION_SIZE: usize = 1024;

#[derive(Debug, Copy, Clone)]
pub struct State {
    pub(crate) allocating: usize,
    pub(crate) deallocating: usize,
    pub(crate) heap_size: usize,
    pub(crate) allocated: usize,
    pub(crate) min_size: usize,
    pub(crate) bump_mode: bool,
    pub(crate) bump_allocated: usize,
    pub(crate) bump_size: usize,
}

impl Allocator {
    pub const fn new() -> Self {
        Self {
            bump: bump::Alloc::new(),
            bump_mode: AtomicBool::new(true),
            allocator: buddy::Alloc::new(32),
            allocating: AtomicUsize::new(0),
            deallocating: AtomicUsize::new(0),
        }
    }

    pub fn state(&self) -> State {
        State {
            allocating: self.allocating.load(Ordering::Acquire),
            deallocating: self.deallocating.load(Ordering::Acquire),
            bump_mode: self.bump_mode.load(Ordering::Acquire),
            heap_size: self.allocator.total_size(),
            allocated: self.allocator.allocated_size(),
            min_size: self.allocator.min_size(),
            bump_allocated: self.bump.allocated_size(),
            bump_size: self.bump.total_size(),
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
        if self.bump_mode.swap(false, Ordering::Release) {
            tracing::debug!("disabled bump allocator mode");
        }
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
        let ptr = if self.bump_mode.load(Ordering::Acquire) {
            GlobalAlloc::alloc(&self.bump, layout)
        } else {
            GlobalAlloc::alloc(&self.allocator, layout)
        };
        self.allocating.fetch_sub(1, Ordering::Release);
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.deallocating.fetch_add(1, Ordering::Release);
        if !self.bump.owns(ptr) {
            GlobalAlloc::dealloc(&self.allocator, ptr, layout);
        }
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

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let &Self {
            allocating,
            deallocating,
            heap_size,
            allocated,
            min_size,
            bump_mode,
            bump_allocated,
            bump_size,
        } = self;
        f.write_str("heap stats:\n")?;
        writeln!(f, "  {allocating} cores allocating")?;
        writeln!(f, "  {deallocating} cores deallocating")?;

        if bump_mode {
            writeln!(f, "  bump allocator mode only")?;
        } else {
            let digits = (heap_size).checked_ilog(10).unwrap_or(0) + 1;
            let free = heap_size - allocated;
            writeln!(f, "buddy heap:")?;

            writeln!(f, "  {free:>digits$} B free", digits = digits)?;

            writeln!(f, "  {heap_size:>digits$} B total", digits = digits)?;
            writeln!(f, "  {free:>digits$} B free", digits = digits)?;
            writeln!(f, "  {allocated:>digits$} B busy", digits = digits)?;
            writeln!(
                f,
                "  {min_size:>digits$} B minimum allocation",
                digits = digits
            )?;
        }

        writeln!(f, "bump region:")?;
        let bump_digits = (bump_size).checked_ilog(10).unwrap_or(0) + 1;
        let bump_free = bump_size - bump_allocated;

        writeln!(f, "  {bump_free:>digits$} B free", digits = bump_digits)?;
        writeln!(
            f,
            "  {bump_allocated:>digits$} B used",
            digits = bump_digits
        )?;
        Ok(())
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
