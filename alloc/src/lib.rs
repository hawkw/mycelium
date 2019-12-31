//! The simplest thing resembling an "allocator" I could possibly create.
//! Allocates into a "large" static array.
#![no_std]

use core::alloc::{Layout, GlobalAlloc};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};


// 640k is enough for anyone
const HEAP_SIZE: usize = 640 * 1024;

#[repr(align(16))]
struct Heap(UnsafeCell<MaybeUninit<[u8; HEAP_SIZE]>>);

unsafe impl Sync for Heap {}

static HEAP: Heap = Heap(UnsafeCell::new(MaybeUninit::uninit()));
static USED: AtomicUsize = AtomicUsize::new(0);

/// NOTABLE INVARIANTS:
///  * `Layout` is non-zero sized (enforced by `GlobalAlloc`)
///  * `align` is a power of two (enforced by `Layout::from_size_align`)
pub unsafe fn alloc(layout: Layout) -> *mut u8 {
    let heap = HEAP.0.get() as *mut u8;

    let mut prev = USED.load(Ordering::Relaxed);
    loop {
        let align_offset = heap.add(prev).align_offset(layout.align());

        // NOTE: layout.size() is non-zero. The check will fail if
        // `align_offset` exceeds `HEAP_SIZE - prev`.
        let space = (HEAP_SIZE - prev).saturating_sub(align_offset);
        if space < layout.size() {
            return ptr::null_mut();
        }

        let next = prev + align_offset + layout.size();
        match USED.compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                return heap.add(prev + align_offset);
            }
            Err(next_prev) => {
                prev = next_prev;
                continue;
            }
        }
    }
}

pub unsafe fn dealloc(_ptr: *mut u8, _layout: Layout) {
    // lol
}

pub struct Alloc;

unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        dealloc(ptr, layout)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::mem;

    #[test]
    fn alloc_small() {
        let p0 = unsafe { alloc(Layout::new::<u32>()) };
        assert!(!p0.is_null());

        let p1 = unsafe { alloc(Layout::new::<u32>()) };
        assert!(!p1.is_null());

        assert_eq!(p0.align_offset(mem::align_of::<u32>()), 0);
        assert_eq!(p1.align_offset(mem::align_of::<u32>()), 0);

        assert_eq!((p1 as usize) - (p0 as usize), 4);
    }

    #[test]
    fn alloc_alignment() {
        let p0 = unsafe { alloc(Layout::new::<u8>()) };
        assert!(!p0.is_null());

        let p1 = unsafe { alloc(Layout::new::<u8>()) };
        assert!(!p1.is_null());

        let p2 = unsafe { alloc(Layout::new::<u32>()) };
        assert!(!p2.is_null());

        assert_eq!((p1 as usize) - (p0 as usize), 1);
        assert!((p2 as usize) - (p1 as usize) > 0);

        assert_eq!(p2.align_offset(mem::align_of::<u32>()), 0);
    }
}
