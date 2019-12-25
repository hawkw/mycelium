use crate::{Address, Architecture};
use core::{cmp, fmt, marker::PhantomData, ops, slice};

pub trait Size: Copy + PartialEq + Eq {
    /// The size (in bits) of this page.
    const SIZE: usize;
}

/// An allocator for physical pages of a given size.
///
/// # Safety
///
/// This trait is unsafe to implement, as implementations are responsible for
/// guaranteeing that allocated pages are unique, and may not be allocated by
/// another page allocator.
pub unsafe trait Alloc<S: Size> {
    type Arch: Architecture;
    /// Allocate a single page.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `alloc_range`.
    ///
    /// # Returns
    /// - `Ok(Page)` if a page was successfully allocated.
    /// - `Err` if no more pages can be allocated by this allocator.
    fn alloc(&mut self) -> Result<Page<<Self::Arch as Architecture>::PAddr, S>, AllocErr> {
        self.alloc_range(1).map(|r| r.start())
    }

    /// Allocate a range of `len` pages.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(
        &mut self,
        len: usize,
    ) -> Result<PageRange<<Self::Arch as Architecture>::PAddr, S>, AllocErr>;

    /// Deallocate a single page.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `dealloc_range`.
    ///
    /// # Returns
    /// - `Ok(())` if the page was successfully deallocated.
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc(
        &mut self,
        page: Page<<Self::Arch as Architecture>::PAddr, S>,
    ) -> Result<(), AllocErr> {
        self.dealloc_range(page.range_inclusive(page))
    }

    /// Deallocate a range of pages.
    ///
    /// # Returns
    /// - `Ok(())` if a range of pages was successfully deallocated
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc_range(
        &self,
        range: PageRange<<Self::Arch as Architecture>::PAddr, S>,
    ) -> Result<(), AllocErr>;
}

/// A memory page.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Page<A, S: Size> {
    base: A,
    _size: PhantomData<S>,
}

/// A range of memory pages of the same size.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageRange<A: Address, S: Size> {
    start: Page<A, S>,
    end: Page<A, S>,
}

pub struct NotAligned<S> {
    _size: PhantomData<S>,
}

pub struct AllocErr {
    // TODO: eliza
    _p: (),
}

// === impl Page ===

impl<A: Address, S: Size> Page<A, S> {
    /// Returns a page starting at the given address.
    pub fn starting_at(addr: impl Into<A>) -> Result<Self, NotAligned<S>> {
        let addr = addr.into();
        if !addr.is_aligned(S::SIZE) {
            return Err(NotAligned { _size: PhantomData });
        }
        Ok(Self::containing(addr))
    }

    /// Returns the page that contains the given address.
    pub fn containing(addr: impl Into<A>) -> Self {
        let base = addr.into().align_down(S::SIZE);
        Self {
            base,
            _size: PhantomData,
        }
    }

    pub fn base_address(&self) -> A {
        self.base
    }

    pub fn end_address(&self) -> A {
        self.base + S::SIZE
    }

    pub fn contains(&self, addr: impl Into<A>) -> bool {
        let addr = addr.into();
        addr >= self.base && addr <= self.end_address()
    }

    pub fn range_inclusive(self, end: Page<A, S>) -> PageRange<A, S> {
        PageRange { start: self, end }
    }

    pub fn range_to(self, end: Page<A, S>) -> PageRange<A, S> {
        PageRange {
            start: self,
            end: end - 1,
        }
    }

    /// Returns the entire contents of the page as a slice.
    ///
    /// # Safety
    ///
    /// When calling this method, ensure that the page will not be mutated
    /// concurrently, including by user code.
    pub unsafe fn as_slice(&self) -> &[u8] {
        let start = self.base.as_ptr() as *const u8;
        slice::from_raw_parts::<u8>(start, S::SIZE)
    }

    /// Returns the entire contents of the page as a mutable slice.
    ///
    /// # Safety
    ///
    /// When calling this method, ensure that the page will not be read or mutated
    /// concurrently, including by user code.
    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        let start = self.base.as_ptr() as *mut u8;
        slice::from_raw_parts_mut::<u8>(start, S::SIZE)
    }
}

impl<A: Address, S: Size> ops::Add<usize> for Page<A, S> {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        Page {
            base: self.base + (S::SIZE * rhs),
            _size: PhantomData,
        }
    }
}

impl<A: Address, S: Size> ops::Sub<usize> for Page<A, S> {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        Page {
            base: self.base - (S::SIZE * rhs),
            _size: PhantomData,
        }
    }
}

impl<A: Address, S: Size> cmp::PartialOrd for Page<A, S> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        // Since the pages are guaranteed by the type system to be the same
        // we can simply compare the base addresses.
        self.base.partial_cmp(&other.base)
    }
}

impl<A: Address, S: Size> cmp::Ord for Page<A, S> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // Since the pages are guaranteed by the type system to be the same
        // we can simply compare the base addresses.
        self.base.cmp(&other.base)
    }
}

impl<A: fmt::Debug, S: Size> fmt::Debug for Page<A, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Page")
            .field("base", &self.base)
            .field("size", &S::SIZE)
            .finish()
    }
}

impl<S> fmt::Debug for NotAligned<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotAligned")
            .field("size", &format_args!("{}", core::any::type_name::<S>()))
            .finish()
    }
}

// === impl PageRange ===

// A PageRange has a minimum size of 1, this will never be empty.
#[allow(clippy::len_without_is_empty)]
impl<A: Address, S: Size> PageRange<A, S> {
    pub fn start(&self) -> Page<A, S> {
        self.start
    }

    pub fn end(&self) -> Page<A, S> {
        self.end
    }

    pub fn len(&self) -> usize {
        unimplemented!("eliza")
    }
}

impl<A: Address, S: Size> Iterator for PageRange<A, S> {
    type Item = Page<A, S>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.start > self.end {
            return None;
        }
        let next = self.start;
        self.start = self.start + 1;
        Some(next)
    }
}
