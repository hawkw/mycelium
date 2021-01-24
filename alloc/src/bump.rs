use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

macro_rules! try_null {
    ($e:expr) => {
        match $e {
            Some(x) => x,
            None => return ptr::null_mut(),
        }
    };
}

// 640k is enough for anyone
const HEAP_SIZE: usize = 640 * 1024;

#[repr(align(16))]
struct Heap(UnsafeCell<MaybeUninit<[u8; HEAP_SIZE]>>);

unsafe impl Sync for Heap {}

static HEAP: Heap = Heap(UnsafeCell::new(MaybeUninit::uninit()));
static FREE: AtomicUsize = AtomicUsize::new(HEAP_SIZE);

/// # Safety
///
/// See [`GlobalAlloc::alloc`]
///
/// NOTABLE INVARIANTS:
///  * `Layout` is non-zero sized (enforced by `GlobalAlloc`)
///  * `align` is a power of two (enforced by `Layout::from_size_align`)
pub unsafe fn alloc(layout: Layout) -> *mut u8 {
    let heap = HEAP.0.get() as *mut u8;

    let mut prev = FREE.load(Ordering::Relaxed);
    loop {
        // Ensure enough space is allocated
        let new_free = try_null!(prev.checked_sub(layout.size()));

        // Ensure the final pointer is aligned
        let new_ptr = (heap as usize + new_free) & !(layout.align() - 1);
        let new_free = try_null!(new_ptr.checked_sub(heap as usize));

        match FREE.compare_exchange_weak(prev, new_free, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return new_ptr as *mut u8,
            Err(next_prev) => {
                prev = next_prev;
                continue;
            }
        }
    }
}

/// Does nothing, because freeing is hard.
///
/// # Safety
///
/// See [`GlobalAlloc::dealloc`]
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

        assert_eq!((p0 as usize) - (p1 as usize), 4);
    }

    #[test]
    fn alloc_alignment() {
        let p0 = unsafe { alloc(Layout::new::<u8>()) };
        assert!(!p0.is_null());

        let p1 = unsafe { alloc(Layout::new::<u8>()) };
        assert!(!p1.is_null());

        let p2 = unsafe { alloc(Layout::new::<u32>()) };
        assert!(!p2.is_null());

        assert_eq!((p0 as usize) - (p1 as usize), 1);
        assert!((p1 as usize) - (p2 as usize) > 0);

        assert_eq!(p2.align_offset(mem::align_of::<u32>()), 0);
    }
}
