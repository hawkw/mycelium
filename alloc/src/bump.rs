//! The simplest thing resembling an "allocator" I could possibly create.
//! Allocates into a "large" static array.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::fmt;
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

pub struct Alloc<const SIZE: usize> {
    heap: Heap<SIZE>,
    free: AtomicUsize,
}

#[repr(align(16))]
struct Heap<const SIZE: usize>(UnsafeCell<MaybeUninit<[u8; SIZE]>>);

impl<const SIZE: usize> Alloc<SIZE> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            heap: Heap(UnsafeCell::new(MaybeUninit::uninit())),
            free: AtomicUsize::new(SIZE),
        }
    }

    #[must_use]
    pub fn total_size(&self) -> usize {
        SIZE
    }

    #[must_use]
    pub fn allocated_size(&self) -> usize {
        SIZE - self.free.load(Ordering::Relaxed)
    }

    pub fn owns(&self, addr: *mut u8) -> bool {
        let start_addr = self.heap.0.get() as *mut u8 as usize;
        let end_addr = start_addr + SIZE;
        (addr as usize) < end_addr
    }
}

impl<const SIZE: usize> Default for Alloc<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<const SIZE: usize> GlobalAlloc for Alloc<SIZE> {
    /// # Safety
    ///
    /// See [`GlobalAlloc::alloc`]
    ///
    /// NOTABLE INVARIANTS:
    ///  * `Layout` is non-zero sized (enforced by `GlobalAlloc`)
    ///  * `align` is a power of two (enforced by `Layout::from_size_align`)
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap = self.heap.0.get() as *mut u8;

        let mut prev = self.free.load(Ordering::Relaxed);
        loop {
            // Ensure enough space is allocated
            let new_free = try_null!(prev.checked_sub(layout.size()));

            // Ensure the final pointer is aligned
            let new_ptr = (heap as usize + new_free) & !(layout.align() - 1);
            let new_free = try_null!(new_ptr.checked_sub(heap as usize));

            match self.free.compare_exchange_weak(
                prev,
                new_free,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
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
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // lol
    }
}

// Safety: access to the bump region is guarded by an atomic.
unsafe impl<const SIZE: usize> Sync for Alloc<SIZE> {}

impl<const SIZE: usize> fmt::Debug for Alloc<SIZE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { free, heap: _ } = self;
        f.debug_struct("bump::Alloc")
            .field("heap", &format_args!("[u8; {SIZE}]"))
            .field("free", &free)
            .field("allocated_size", &self.allocated_size())
            .finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::mem;

    #[test]
    fn alloc_small() {
        static ALLOC: Alloc<1024> = Alloc::new();
        let p0 = unsafe { ALLOC.alloc(Layout::new::<u32>()) };
        assert!(!p0.is_null());

        let p1 = unsafe { ALLOC.alloc(Layout::new::<u32>()) };
        assert!(!p1.is_null());

        assert_eq!(p0.align_offset(mem::align_of::<u32>()), 0);
        assert_eq!(p1.align_offset(mem::align_of::<u32>()), 0);

        assert_eq!((p0 as usize) - (p1 as usize), 4);
    }

    #[test]
    fn alloc_alignment() {
        static ALLOC: Alloc<1024> = Alloc::new();
        let p0 = unsafe { ALLOC.alloc(Layout::new::<u8>()) };
        assert!(!p0.is_null());

        let p1 = unsafe { ALLOC.alloc(Layout::new::<u8>()) };
        assert!(!p1.is_null());

        let p2 = unsafe { ALLOC.alloc(Layout::new::<u32>()) };
        assert!(!p2.is_null());

        assert_eq!((p0 as usize) - (p1 as usize), 1);
        assert!((p1 as usize) - (p2 as usize) > 0);

        assert_eq!(p2.align_offset(mem::align_of::<u32>()), 0);
    }
}
