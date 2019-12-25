use alloc::alloc::{Alloc, AllocErr, Layout};
use core::{
    fmt, iter, mem,
    ptr::{self, NonNull},
};
use hal_core::{
    mem::page::{self, Page},
    Address,
};

#[derive(Clone)]
#[repr(transparent)]
pub struct BumpPage(NonNull<Header>);

#[repr(C)]
#[derive(Debug)]
struct Header {
    top: *mut u8,
    ptr: *mut u8,
    end: *mut u8,
    next: Option<BumpPage>,
}

impl Header {
    #[inline(always)]
    fn allocate_in_page(&mut self, layout: Layout) -> Option<NonNull<u8>> {
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

    fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        self.allocate_in_page(layout)
            .or_else(|| self.alloc_next(layout))
    }

    fn alloc_next(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let next = unsafe { self.next.as_mut()?.0.as_mut() };
        next.allocate(layout)
    }
}

unsafe impl Alloc for Header {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocErr> {
        self.allocate(layout).ok_or_else(|| AllocErr)
    }

    unsafe fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
        // let ptr = ptr.as_ptr();
        // assert!(
        //     ptr >= self.end,
        //     "tried to deallocate an allocation ({:#p}) that did not come from this allocator!",
        //     ptr
        // );
        // assert!(
        //     ptr <= self.top,
        //     "tried to deallocate an allocation ({:#p}) that did not come from this allocator!",
        //     ptr
        // );
        // nop
    }
}

unsafe impl Alloc for BumpPage {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocErr> {
        self.header_mut().alloc(layout)
    }

    unsafe fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        self.header_mut().dealloc(ptr, layout)
    }
}

impl BumpPage {
    pub fn new<A, S>(page: Page<A, S>) -> Self
    where
        A: Address,
        S: page::Size,
    {
        let top = {
            let page_end = page.end_address().as_ptr() as *mut u8;
            let sz = {
                let sz = mem::size_of::<Header>();
                if sz > core::isize::MAX as usize {
                    unreachable!("this shouldn't happen!");
                } else {
                    sz as isize
                }
            };
            unsafe { page_end.offset(-sz) }
        };
        let end = page.base_address().as_ptr() as *mut _;
        let ptr = top;
        unsafe {
            ptr::write(
                ptr as *mut Header,
                Header {
                    top,
                    end,
                    ptr,
                    next: None,
                },
            );
        }
        let page_ptr = NonNull::new(ptr)
            .expect("page was zero-sized!")
            .cast::<Header>();
        Self(page_ptr)
    }

    pub fn as_dyn_alloc(mut self) -> NonNull<dyn Alloc> {
        let header = self.header_mut() as &mut dyn Alloc;
        NonNull::from(header)
    }

    fn last_page(&mut self) -> Self {
        let mut current = self.clone();
        while let Some(next) = current.header_mut().next.clone() {
            current = next;
        }
        current
    }

    fn set_next(&mut self, next: Self) {
        self.header_mut().next = Some(next);
    }

    fn header_mut(&mut self) -> &mut Header {
        unsafe { self.0.as_mut() }
    }

    fn header(&self) -> &Header {
        unsafe { self.0.as_ref() }
    }
}

impl<A, S> iter::Extend<Page<A, S>> for BumpPage
where
    A: Address,
    S: page::Size,
{
    fn extend<T: IntoIterator<Item = Page<A, S>>>(&mut self, iter: T) {
        let mut current = self.last_page();
        for page in iter.into_iter() {
            let next = Self::new(page);
            current.set_next(next.clone());
            current = next
        }
    }
}

impl<A, S> iter::FromIterator<Page<A, S>> for BumpPage
where
    A: Address,
    S: page::Size,
{
    fn from_iter<T: IntoIterator<Item = Page<A, S>>>(iter: T) -> Self {
        let mut iter = iter.into_iter();
        let mut this = Self::new(iter.next().unwrap());
        this.extend(iter);
        this
    }
}

impl fmt::Debug for BumpPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BumpPage")
            .field("ptr", &self.0)
            .field("header", self.header())
            .finish()
    }
}
