#![cfg_attr(target_os = "none", no_std)]
#![feature(allocator_api)]
#![feature(const_fn)]
extern crate alloc;
pub mod bump;

use alloc::alloc::{Alloc, AllocErr, GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use mycelium_util::sync::spin;

#[derive(Debug)]
pub struct LockAlloc {
    inner: spin::Mutex<*mut dyn Alloc>,
}

const NOP_ALLOC: NopAlloc = NopAlloc;
struct NopAlloc;

impl LockAlloc {
    pub const fn none() -> Self {
        Self {
            inner: spin::Mutex::new(&NOP_ALLOC as &dyn Alloc as *const _ as *mut _),
        }
    }

    pub unsafe fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocErr> {
        let lock = self.inner.lock();
        if let Some(alloc) = lock.as_mut() {
            alloc.alloc(layout)
        } else {
            // TODO(eliza): log that no allocator exists
            Err(AllocErr)
        }
    }

    pub unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let lock = self.inner.lock();
        if let Some(alloc) = lock.as_mut() {
            alloc.dealloc(ptr, layout)
        } else {
            panic!("tried to deallocate {:#p}, but no allocator exists");
        }
    }

    pub fn set_allocator(&self, new_alloc: NonNull<dyn Alloc>) {
        let mut lock = self.inner.lock();
        *lock = new_alloc.as_ptr();
    }
}

unsafe impl Alloc for NopAlloc {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocErr> {
        Err(AllocErr)
    }

    unsafe fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
        unreachable!()
    }
}
unsafe impl GlobalAlloc for LockAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout)
            .map(NonNull::as_ptr)
            .unwrap_or_else(|_| ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let ptr = NonNull::new(ptr).expect("tried to deallocate a null pointer!");
        self.deallocate(ptr, layout);
    }
}
