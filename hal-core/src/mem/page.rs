use crate::{Address, PAddr, VAddr};
use core::{cmp, ops, slice};
use mycelium_util::fmt;

pub trait Size: Copy + Eq + PartialEq + fmt::Display {
    /// Returns the size (in bytes) of this page.
    fn as_usize(&self) -> usize;
}

/// A statically known page size.
pub trait StaticSize: Copy + Eq + PartialEq + fmt::Display {
    /// The size (in bytes) of this page.
    const SIZE: usize;
    const PRETTY_NAME: &'static str;
    const INSTANCE: Self;
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
    fn alloc(&self, size: S) -> Result<Page<PAddr, S>, AllocErr> {
        self.alloc_range(size, 1).map(|r| r.start())
    }

    /// Allocate a range of `len` pages.
    ///
    /// # Returns
    /// - `Ok(PageRange)` if a range of pages was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&self, size: S, len: usize) -> Result<PageRange<PAddr, S>, AllocErr>;

    /// Deallocate a single page.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `dealloc_range`.
    ///
    /// # Returns
    /// - `Ok(())` if the page was successfully deallocated.
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc(&self, page: Page<PAddr, S>) -> Result<(), AllocErr> {
        self.dealloc_range(page.range_inclusive(page))
    }

    /// Deallocate a range of pages.
    ///
    /// # Returns
    /// - `Ok(())` if a range of pages was successfully deallocated
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc_range(&self, range: PageRange<PAddr, S>) -> Result<(), AllocErr>;
}

pub trait Map<S, A>
where
    S: Size,
    A: Alloc<S>,
{
    type Entry: PageFlags<S>;

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
        &mut self,
        virt: Page<VAddr, S>,
        phys: Page<PAddr, S>,
        frame_alloc: &mut A,
    ) -> Handle<'_, S, Self::Entry>;

    fn flags_mut(&mut self, virt: Page<VAddr, S>) -> Handle<'_, S, Self::Entry>;

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
    unsafe fn unmap(&mut self, virt: Page<VAddr, S>) -> Page<PAddr, S>;

    /// Identity map the provided physical page to the virtual page with the
    /// same address.
    fn identity_map(
        &mut self,
        phys: Page<PAddr, S>,
        frame_alloc: &mut A,
    ) -> Handle<'_, S, Self::Entry> {
        let base_paddr = phys.base_addr().as_usize();
        let virt = Page::containing(VAddr::from_usize(base_paddr), phys.size());
        unsafe { self.map_page(virt, phys, frame_alloc) }
    }

    /// Map the range of virtual memory pages represented by `virt` to the range
    /// of physical pages represented by `phys`.
    ///
    /// # Arguments
    ///
    /// - `virt`: the range of virtual pages to map.
    /// - `phys`: the range of physical pages to map `virt` to.
    /// - `set_flags`: a closure invoked with a `Handle` to each page in the
    ///   range as it is mapped. This closure may modify the flags for that page
    ///   before the changes to the page mapping are committed.
    ///
    ///   **Note**: The [`Handle::virt_page`] method may be used to determine
    ///   which page's flags would be modified by each invocation of the cosure.
    /// - `frame_alloc`: a page-frame allocator.
    ///
    /// # Panics
    ///
    /// - If the two ranges have different lengths.
    /// - If the size is dynamic and the two ranges are of different sized pages.
    /// - If the physical address is invalid.
    /// - If any page is already mapped.
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
    unsafe fn map_range<F>(
        &mut self,
        virt: PageRange<VAddr, S>,
        phys: PageRange<PAddr, S>,
        mut set_flags: F,
        frame_alloc: &mut A,
    ) -> PageRange<VAddr, S>
    where
        F: FnMut(&mut Handle<'_, S, Self::Entry>),
    {
        let _span = tracing::trace_span!("map_range", ?virt, ?phys).entered();
        assert_eq!(
            virt.len(),
            phys.len(),
            "virtual and physical page ranges must have the same number of pages"
        );
        assert_eq!(
            virt.size(),
            phys.size(),
            "virtual and physical pages must be the same size"
        );
        for (virt, phys) in virt.into_iter().zip(&phys) {
            tracing::trace!(virt.page = ?virt, phys.page = ?phys, "mapping...");
            let mut flags = self.map_page(virt, phys, frame_alloc);
            set_flags(&mut flags);
            flags.commit();
            tracing::trace!(virt.page = ?virt, phys.page = ?phys, "mapped");
        }
        virt
    }

    /// Unmap the provided range of virtual pages.
    ///
    /// This does not deallocate any page frames.
    ///
    /// # Notes
    ///
    /// The default implementation of this method does *not* assume that the
    /// virtual pages are mapped to a contiguous range of physical page frames.
    /// Overridden implementations *may* perform different behavior when the
    /// pages are mapped contiguously in the physical address space, but they
    /// *must not* assume this. If an implementation performs additional
    /// behavior for contiguously-mapped virtual page ranges, it must check that
    /// the page range is, in fact, contiguously mapped.
    ///
    /// Additionally, and unlike [`unmap`], this method does not return
    /// a physical [`PageRange`], since it is not guaranteed that the unmapped
    /// pages are mapped to a contiguous physical page range.
    ///
    /// # Panics
    ///
    /// - If any virtual page in the range was not mapped.
    ///
    /// # Safety
    ///
    /// Manual control of page mappings may be used to violate Rust invariants
    /// in a variety of exciting ways.
    ///
    /// [`unmap`]: Self::unmap
    unsafe fn unmap_range(&mut self, virt: PageRange<VAddr, S>) {
        let _span = tracing::trace_span!("unmap_range", ?virt).entered();

        for virt in &virt {
            self.unmap(virt);
        }
    }

    /// Identity map the provided physical page range to a range of virtual
    /// pages with the same address
    ///
    /// # Arguments
    ///
    /// - `phys`: the range of physical pages to identity map
    /// - `set_flags`: a closure invoked with a `Handle` to each page in the
    ///   range as it is mapped. This closure may modify the flags for that page
    ///   before the changes to the page mapping are committed.
    ///
    ///   **Note**: The [`Handle::virt_page`] method may be used to determine
    ///   which page's flags would be modified by each invocation of the cosure.
    /// - `frame_alloc`: a page-frame allocator.
    ///
    /// # Panics
    ///
    /// - If any page's physical address is invalid.
    /// - If any page is already mapped.
    fn identity_map_range<F>(
        &mut self,
        phys: PageRange<PAddr, S>,
        set_flags: F,
        frame_alloc: &mut A,
    ) -> PageRange<VAddr, S>
    where
        F: FnMut(&mut Handle<'_, S, Self::Entry>),
    {
        let base_paddr = phys.base_addr().as_usize();
        let page_size = phys.start().size();
        let virt_base = Page::containing(VAddr::from_usize(base_paddr), page_size);
        let end_paddr = phys.end_addr().as_usize();
        let virt_end = Page::containing(VAddr::from_usize(end_paddr), page_size);
        let virt = virt_base.range_to(virt_end);
        unsafe { self.map_range(virt, phys, set_flags, frame_alloc) }
    }
}

impl<M, A, S> Map<S, A> for &mut M
where
    M: Map<S, A>,
    S: Size,
    A: Alloc<S>,
{
    type Entry = M::Entry;

    #[inline]
    unsafe fn map_page(
        &mut self,
        virt: Page<VAddr, S>,
        phys: Page<PAddr, S>,
        frame_alloc: &mut A,
    ) -> Handle<'_, S, Self::Entry> {
        (*self).map_page(virt, phys, frame_alloc)
    }

    #[inline]
    fn flags_mut(&mut self, virt: Page<VAddr, S>) -> Handle<'_, S, Self::Entry> {
        (*self).flags_mut(virt)
    }

    #[inline]
    unsafe fn unmap(&mut self, virt: Page<VAddr, S>) -> Page<PAddr, S> {
        (*self).unmap(virt)
    }

    #[inline]
    fn identity_map(
        &mut self,
        phys: Page<PAddr, S>,
        frame_alloc: &mut A,
    ) -> Handle<'_, S, Self::Entry> {
        (*self).identity_map(phys, frame_alloc)
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
    unsafe fn set_writable(&mut self, writable: bool);

    /// Set whether or not this page is executable.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    /// Using `set_executable` to make writable memory executable may cause
    /// undefined behavior. Also, this can be used to execute the contents of
    /// arbitrary memory, which (of course) is wildly unsafe.
    unsafe fn set_executable(&mut self, executable: bool);

    /// Set whether or not this page is present.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    unsafe fn set_present(&mut self, present: bool);

    fn is_writable(&self) -> bool;
    fn is_executable(&self) -> bool;
    fn is_present(&self) -> bool;

    /// Commit the changes to the page table.
    ///
    /// Depending on the CPU architecture, this may be a nop. In other cases, it
    /// may invoke special instructions (such as `invlpg` on x86) or write data
    /// to the page table.
    ///
    /// If page table changes are reflected as soon as flags are modified, the
    /// implementation may do nothing.
    fn commit(&mut self, page: Page<VAddr, S>);
}

/// A page in the process of being remapped.
///
/// This reference allows updating page table flags prior to committing changes.
#[derive(Debug)]
#[must_use = "page table updates may not be reflected until changes are committed (using `Handle::commit`)"]
pub struct Handle<'mapper, S: Size, E: PageFlags<S>> {
    entry: &'mapper mut E,
    page: Page<VAddr, S>,
}

/// A memory page.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Page<A, S: Size> {
    base: A,
    size: S,
}

/// A range of memory pages of the same size.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd)]
pub struct PageRange<A: Address, S: Size> {
    start: Page<A, S>,
    end: Page<A, S>,
}

#[derive(Debug, Default)]
pub struct EmptyAlloc {
    _p: (),
}
pub struct NotAligned<S> {
    size: S,
}

#[derive(Debug)]
pub struct AllocErr {
    // TODO: eliza
    _p: (),
}

#[derive(Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum TranslateError<S: Size> {
    NotMapped,
    WrongSize(S),
    Other(&'static str),
}

// === impl Page ===

impl<A: Address, S: StaticSize> Page<A, S> {
    /// Returns a page starting at the given address.
    pub fn starting_at_fixed(addr: impl Into<A>) -> Result<Self, NotAligned<S>> {
        Self::starting_at(addr, S::INSTANCE)
    }

    /// Returns the page that contains the given address.
    pub fn containing_fixed(addr: impl Into<A>) -> Self {
        Self::containing(addr, S::INSTANCE)
    }
}

impl<A: Address, S: Size> Page<A, S> {
    /// Returns a page starting at the given address.
    pub fn starting_at(addr: impl Into<A>, size: S) -> Result<Self, NotAligned<S>> {
        let addr = addr.into();
        if !addr.is_aligned(size.as_usize()) {
            return Err(NotAligned { size });
        }
        Ok(Self::containing(addr, size))
    }

    /// Returns the page that contains the given address.
    pub fn containing(addr: impl Into<A>, size: S) -> Self {
        let base = addr.into().align_down(size.as_usize());
        Self { base, size }
    }

    pub fn base_addr(&self) -> A {
        self.base
    }

    /// Returns the last address in the page, exclusive.
    ///
    /// The returned address will be the base address of the next page.
    pub fn end_addr(&self) -> A {
        self.base + (self.size.as_usize() - 1)
    }

    pub fn size(&self) -> S {
        self.size
    }

    pub fn contains(&self, addr: impl Into<A>) -> bool {
        let addr = addr.into();
        addr >= self.base && addr < self.end_addr()
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
        slice::from_raw_parts::<u8>(start, self.size.as_usize())
    }

    /// Returns the entire contents of the page as a mutable slice.
    ///
    /// # Safety
    ///
    /// When calling this method, ensure that the page will not be read or mutated
    /// concurrently, including by user code.
    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        let start = self.base.as_ptr::<u8>() as *mut _;
        slice::from_raw_parts_mut::<u8>(start, self.size.as_usize())
    }
}

impl<A: Address, S: Size> ops::Add<usize> for Page<A, S> {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        Page {
            base: self.base + (self.size.as_usize() * rhs),
            ..self
        }
    }
}

impl<A: Address, S: Size> ops::Sub<usize> for Page<A, S> {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        Page {
            base: self.base - (self.size.as_usize() * rhs),
            ..self
        }
    }
}

impl<A: Address, S: Size> cmp::PartialOrd for Page<A, S> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        if self.size == other.size {
            self.base.partial_cmp(&other.base)
        } else {
            // XXX(eliza): does it make sense to compare pages of different sizes?
            None
        }
    }
}

impl<A: Address, S: StaticSize> cmp::Ord for Page<A, S> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.base.cmp(&other.base)
    }
}

impl<A: fmt::Debug, S: Size + fmt::Display> fmt::Debug for Page<A, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { base, size } = self;
        f.debug_struct("Page")
            .field("base", base)
            .field("size", &format_args!("{size}"))
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

    /// Returns the base address of the first page in the range.
    pub fn base_addr(&self) -> A {
        self.start.base_addr()
    }

    /// Returns the end address on the last page in the range.
    ///
    /// This is the base address of the page immediately following this range.
    pub fn end_addr(&self) -> A {
        self.end.end_addr()
    }

    /// Returns the size of the pages in the range. All pages in a page range
    /// have the same size.
    #[track_caller]
    pub fn page_size(&self) -> S {
        debug_assert_eq!(self.start.size().as_usize(), self.end.size().as_usize());
        self.start.size()
    }

    pub fn len(&self) -> usize {
        self.size() / self.page_size().as_usize()
    }

    /// Returns the size in bytes of the page range.
    #[track_caller]
    pub fn size(&self) -> usize {
        let diff = self.start.base_addr().difference(self.end.end_addr());
        debug_assert!(
            diff >= 0,
            "assertion failed: page range base address must be lower than end \
            address!\n\
            \x20 base addr = {:?}\n\
            \x20  end addr = {:?}\n\
            ",
            self.base_addr(),
            self.end_addr(),
        );
        // add 1 to compensate for the base address not being included in `difference`
        let diff = diff as usize + 1;
        debug_assert!(
            diff >= self.page_size().as_usize(),
            "assertion failed: page range must be at least one page!\n\
            \x20 difference = {}\n\
            \x20       size = {}\n\
            \x20  base addr = {:?}\n\
            \x20   end addr = {:?}\n\
            ",
            diff,
            self.page_size().as_usize(),
            self.base_addr(),
            self.end_addr(),
        );
        diff
    }
}

impl<A: Address, S: Size> IntoIterator for &'_ PageRange<A, S> {
    type IntoIter = PageRange<A, S>;
    type Item = Page<A, S>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        *self
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

impl<A, S> fmt::Debug for PageRange<A, S>
where
    A: Address + fmt::Debug,
    S: Size + fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { start, end } = self;
        f.debug_struct("PageRange")
            .field("start", start)
            .field("end", end)
            .finish()
    }
}

unsafe impl<S: Size> Alloc<S> for EmptyAlloc {
    fn alloc_range(&self, _: S, _len: usize) -> Result<PageRange<PAddr, S>, AllocErr> {
        Err(AllocErr { _p: () })
    }

    fn dealloc_range(&self, _range: PageRange<PAddr, S>) -> Result<(), AllocErr> {
        Err(AllocErr { _p: () })
    }
}

// === impl NotAligned ===

impl<S: Size + fmt::Display> fmt::Debug for NotAligned<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { size } = self;
        f.debug_struct("NotAligned")
            .field("size", &fmt::display(size))
            .finish()
    }
}

// === impl TranslateError ===

impl<S: Size> From<&'static str> for TranslateError<S> {
    fn from(msg: &'static str) -> Self {
        TranslateError::Other(msg)
    }
}

impl<S: Size + fmt::Display> fmt::Debug for TranslateError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TranslateError::Other(msg) => {
                f.debug_tuple("TranslateError::Other").field(&msg).finish()
            }
            TranslateError::NotMapped => f.debug_tuple("TranslateError::NotMapped").finish(),
            TranslateError::WrongSize(s) => f
                .debug_tuple("TranslateError::WrongSize")
                .field(&format_args!("{s}"))
                .finish(),
        }
    }
}

impl<S: Size> fmt::Display for TranslateError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TranslateError::Other(msg) => write!(f, "error translating page/address: {msg}"),
            TranslateError::NotMapped => f.pad("error translating page/address: not mapped"),
            TranslateError::WrongSize(_) => write!(
                f,
                "error translating page: mapped page is a different size ({})",
                core::any::type_name::<S>()
            ),
        }
    }
}

// === impl AllocErr ===

impl AllocErr {
    pub fn oom() -> Self {
        Self { _p: () }
    }
}

impl<S: Size> From<NotAligned<S>> for AllocErr {
    fn from(_na: NotAligned<S>) -> Self {
        Self { _p: () } // TODO(eliza)
    }
}

impl<S: Size> mycelium_util::error::Error for TranslateError<S> {}

impl<S> Size for S
where
    S: StaticSize,
{
    fn as_usize(&self) -> usize {
        Self::SIZE
    }
}

// === impl Handle ===

impl<'mapper, S, E> Handle<'mapper, S, E>
where
    S: Size,
    E: PageFlags<S>,
{
    pub fn new(page: Page<VAddr, S>, entry: &'mapper mut E) -> Self {
        Self { entry, page }
    }

    /// Returns the virtual page this entry is currently mapped to.
    pub fn virt_page(&self) -> &Page<VAddr, S> {
        &self.page
    }

    /// Set whether or not this page is writable.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    /// Using `set_writable` to make memory that the Rust compiler expects to be
    /// read-only may cause undefined behavior. Making a page which is aliased
    /// page table (i.e. it has multiple page table entries pointing to it) may
    /// also cause undefined behavior.
    #[inline]
    pub unsafe fn set_writable(&mut self, writable: bool) -> &mut Self {
        self.entry.set_writable(writable);
        self
    }

    /// Set whether or not this page is executable.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    /// Using `set_executable` to make writable memory executable may cause
    /// undefined behavior. Also, this can be used to execute the contents of
    /// arbitrary memory, which (of course) is wildly unsafe.
    #[inline]
    pub unsafe fn set_executable(&mut self, executable: bool) -> &mut Self {
        self.entry.set_executable(executable);
        self
    }

    /// Set whether or not this page is present.
    ///
    /// # Safety
    ///
    /// Manual control of page flags can be used to violate Rust invariants.
    #[inline]
    pub unsafe fn set_present(&mut self, present: bool) -> &mut Self {
        self.entry.set_present(present);
        self
    }

    #[inline]
    pub fn is_writable(&self) -> bool {
        self.entry.is_writable()
    }

    #[inline]
    pub fn is_executable(&self) -> bool {
        self.entry.is_executable()
    }

    #[inline]
    pub fn is_present(&self) -> bool {
        self.entry.is_present()
    }

    #[inline]
    pub fn commit(self) -> Page<VAddr, S> {
        self.entry.commit(self.page);
        self.page
    }
}
