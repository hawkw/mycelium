use crate::{Address, PAddr, VAddr};
use core::{cmp, fmt, marker::PhantomData, ops, slice};

pub trait Size: Copy + Eq + PartialEq {
    /// The size (in bits) of this page.
    const SIZE: usize;
    const PRETTY_NAME: &'static str;
}

pub type TranslateResult<A, S> = Result<Page<A, S>, TranslateError<S>>;
/// An allocator for physical pages of a given size.
///
/// # Safety
///
/// This trait is unsafe to implement, as implementations are responsible for
/// guaranteeing that allocated pages are unique, and may not be allocated by
/// another page allocator.
pub unsafe trait Alloc<S: Size> {
    /// Allocate a single page.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `alloc_range`.
    ///
    /// # Returns
    /// - `Ok(Page)` if a page was successfully allocated.
    /// - `Err` if no more pages can be allocated by this allocator.
    fn alloc(&mut self) -> Result<Page<PAddr, S>, AllocErr> {
        self.alloc_range(1).map(|r| r.start())
    }

    /// Allocate a range of `len` pages.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&mut self, len: usize) -> Result<PageRange<PAddr, S>, AllocErr>;

    /// Deallocate a single page.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `dealloc_range`.
    ///
    /// # Returns
    /// - `Ok(())` if the page was successfully deallocated.
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc(&mut self, page: Page<PAddr, S>) -> Result<(), AllocErr> {
        self.dealloc_range(page.range_inclusive(page))
    }

    /// Deallocate a range of pages.
    ///
    /// # Returns
    /// - `Ok(())` if a range of pages was successfully deallocated
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc_range(&self, range: PageRange<PAddr, S>) -> Result<(), AllocErr>;
}

pub trait Map<'mapper, S, A>
where
    S: Size,
    A: Alloc<S>,
{
    type Handle: PageFlags<S>;
    /// Map the virtual memory page represented by `virt` to the physical page
    /// represented bt `phys`.
    ///
    /// # Panics
    ///
    /// - If the physical address is invalid.
    /// - If the page is already mapped.
    ///
    /// # Safety
    ///
    /// Manual control of page mappings may be used to violate Rust invariants
    /// in a variety of exciting ways. For example, aliasing a physical page by
    /// mapping multiple virtual pages to it and setting one or more of those
    /// virtual pages as writable may result in undefined behavior.
    ///
    /// Some rules of thumb:
    ///
    /// - Ensure that the writable XOR executable rule is not violated (by
    ///   making a page both writable and executable).
    /// - Don't alias stack pages onto the heap or vice versa.
    /// - If loading arbitrary code into executable pages, ensure that this code
    ///   is trusted and will not violate the kernel's invariants.
    ///
    /// Good luck and have fun!
    unsafe fn map_page(
        &'mapper mut self,
        virt: Page<VAddr, S>,
        phys: Page<PAddr, S>,
        frame_alloc: &mut A,
    ) -> Self::Handle;

    fn flags_mut(&'mapper mut self, virt: Page<VAddr, S>) -> Self::Handle;

    /// Unmap the provided virtual page, returning the physical page it was
    /// previously mapped to.
    ///
    /// This does not deallocate any page frames.
    ///
    /// # Panics
    ///
    /// - If the virtual page was not mapped.
    ///
    /// # Safety
    ///
    /// Manual control of page mappings may be used to violate Rust invariants
    /// in a variety of exciting ways.
    unsafe fn unmap(&'mapper mut self, virt: Page<VAddr, S>) -> Page<PAddr, S>;

    /// Identity map the provided physical page to the virtual page with the
    /// same address.
    fn identity_map(&'mapper mut self, phys: Page<PAddr, S>, frame_alloc: &mut A) -> Self::Handle {
        let base_paddr = phys.base_address().as_usize();
        let virt = Page::containing(VAddr::from_usize(base_paddr));
        unsafe { self.map_page(virt, phys, frame_alloc) }
    }
}

pub trait TranslatePage<S: Size> {
    fn translate_page(&self, virt: Page<VAddr, S>) -> TranslateResult<PAddr, S>;
}

pub trait TranslateAddr {
    fn translate_addr(&self, addr: VAddr) -> Option<PAddr>;
}

pub trait PageFlags<S: Size> {
    /// Set whether or not this page is writable.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    /// Using `set_writable` to make memory that the Rust compiler expects to be
    /// read-only may cause undefined behavior. Making a page which is aliased
    /// page table (i.e. it has multiple page table entries pointing to it) may
    /// also cause undefined behavior.
    unsafe fn set_writable(&mut self, writable: bool) -> &mut Self;

    /// Set whether or not this page is executable.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    /// Using `set_executable` to make writable memory executable may cause
    /// undefined behavior. Also, this can be used to execute the contents of
    /// arbitrary memory, which (of course) is wildly unsafe.
    unsafe fn set_executable(&mut self, executable: bool) -> &mut Self;

    /// Set whether or not this page is present.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    unsafe fn set_present(&mut self, present: bool) -> &mut Self;

    fn is_writable(&self) -> bool;
    fn is_executable(&self) -> bool;
    fn is_present(&self) -> bool;

    fn commit(self) -> Page<VAddr, S>;
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

#[derive(Debug, Default)]
pub struct EmptyAlloc {
    _p: (),
}
pub struct NotAligned<S> {
    _size: PhantomData<S>,
}

pub struct AllocErr {
    // TODO: eliza
    _p: (),
}

#[derive(Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum TranslateError<S: Size> {
    NotMapped,
    WrongSize(PhantomData<S>),
    Other(&'static str),
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
        let start = self.base.as_ptr::<u8>() as *mut _;
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

unsafe impl<S: Size> Alloc<S> for EmptyAlloc {
    fn alloc_range(&mut self, _len: usize) -> Result<PageRange<PAddr, S>, AllocErr> {
        Err(AllocErr { _p: () })
    }

    fn dealloc_range(&self, _range: PageRange<PAddr, S>) -> Result<(), AllocErr> {
        Err(AllocErr { _p: () })
    }
}

// === impl NotAligned ===

impl<S: Size> fmt::Debug for NotAligned<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotAligned")
            .field("size", &S::SIZE)
            .finish()
    }
}

// === impl TranslateError ===

impl<S: Size> From<&'static str> for TranslateError<S> {
    fn from(msg: &'static str) -> Self {
        TranslateError::Other(msg)
    }
}

impl<S: Size> fmt::Debug for TranslateError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TranslateError::Other(msg) => {
                f.debug_tuple("TranslateError::Other").field(&msg).finish()
            }
            TranslateError::NotMapped => f.debug_tuple("TranslateError::NotMapped").finish(),
            TranslateError::WrongSize(_) => f
                .debug_tuple("TranslateError::WrongSize")
                .field(&S::PRETTY_NAME)
                .finish(),
        }
    }
}

impl<S: Size> fmt::Display for TranslateError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TranslateError::Other(msg) => write!(f, "error translating page/address: {}", msg),
            TranslateError::NotMapped => f.pad("error translating page/address: not mapped"),
            TranslateError::WrongSize(_) => write!(
                f,
                "error translating page: mapped page is a different size ({})",
                core::any::type_name::<S>()
            ),
        }
    }
}

impl<S: Size> mycelium_util::error::Error for TranslateError<S> {}
