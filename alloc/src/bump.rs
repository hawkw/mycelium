use alloc::alloc::{Alloc, AllocErr, Layout};
use core::cell::Cell;
use core::ptr::NonNull;

#[derive(Debug)]
#[repr(transparent)]
pub struct BumpPage(NonNull<Page>);

#[repr(C)]
struct Page {
    header: Header,
    page: [u8],
}

#[repr(C)]
#[derive(Debug)]
struct Header {
    ptr: *mut u8,
    end: *mut u8,
    next: Option<BumpPage>,
}

impl Page {
    #[inline(always)]
    fn alloc_in_page(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size();
        let align = layout.align();
        if size == 0 {
            unsafe {
                return Some(NonNull::new_unchecked(align as *mut u8));
            }
        }

        let ptr = (self.ptr as usize).checked_sub(size)?;
        let ptr = ptr & !(align - 1);

        if ptr < self.end as usize {
            return None;
        }

        let ptr = ptr as *mut u8;
        self.ptr = ptr;
        unsafe { Some(NonNull::new_unchecked(ptr)) }
    }
}

impl Alloc for BumpPage {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocErr> {
        unimplemented!()
    }
}
