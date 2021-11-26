use alloc::alloc::{GlobalAlloc, Layout};
use mycelium_alloc::{buddy, bump};
use mycelium_util::fmt;

pub struct Heap {
    bump_region: bump::Alloc,
    allocator: &'static buddy::Alloc,
}

impl Heap {
    pub const fn new(allocator: &'static buddy::Alloc) -> Self {
        Self {
            bump_region: bump::Alloc,
            allocator,
        }
    }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let bump_ptr = self.bump_region.alloc(layout);
        if bump_ptr.is_null() {
            return self.allocator.alloc(layout);
        }

        tracing::trace!(?layout, ptr = ?fmt::ptr(bump_ptr), "allocated from bump pointer region");
        bump_ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if self.bump_region.owns(ptr) {
            tracing::debug!(?layout, ptr = ?fmt::ptr(ptr), "deallocating bump region pointer, doing nothing lol");
            return;
        }

        self.allocator.dealloc(ptr, layout)
    }
}

mycelium_util::decl_test! {
    fn basic_alloc() {
        // Let's allocate something, for funsies
        use alloc::vec::Vec;
        let mut v = Vec::new();
        tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
        v.push(5u64);
        tracing::info!(vec = ?v, vec.addr = ?v.as_ptr());
        v.push(10u64);
        tracing::info!(vec=?v, vec.addr=?v.as_ptr());
        assert_eq!(v.pop(), Some(10));
        assert_eq!(v.pop(), Some(5));
    }
}

mycelium_util::decl_test! {
    fn alloc_big() {
        use alloc::vec::Vec;
        let mut v = Vec::new();

        for i in 0..2048 {
            v.push(i);
        }

        tracing::info!(vec = ?v);
    }
}
