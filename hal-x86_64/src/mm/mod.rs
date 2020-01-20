use self::size::*;
use crate::{PAddr, VAddr};
use core::{
    fmt,
    marker::PhantomData,
    ops,
    ptr::{self, NonNull},
};
use hal_core::{
    mem::page::{self, Page, Size, TranslateAddr, TranslateError, TranslatePage, TranslateResult},
    Address,
};

const ENTRIES: usize = 512;

type VirtPage<S> = Page<VAddr, S>;
type PhysPage<S> = Page<PAddr, S>;

/// An x86_64 page table.
#[repr(align(4096))]
#[repr(C)]
pub struct PageTable<L> {
    entries: [Entry<L>; ENTRIES],
}

#[derive(Clone)]
#[repr(transparent)]
pub struct Entry<L> {
    entry: u64,
    _level: PhantomData<L>,
}
#[must_use = "page table updates will not be reflected until the changed pages are flushed from the TLB"]
pub struct PageHandle {
    entry: *mut u64,
}

pub fn init_paging(phys_offset: VAddr) {
    let span = tracing::info_span!("paging::init", ?phys_offset);
    let _e = span.enter();

    tracing::info!("initializing paging...");
    let pml4 = unsafe { PageTable::<level::Pml4>::current(phys_offset) };
    let mut present_entries = 0;
    for (idx, entry) in (&pml4.entries[..]).iter().enumerate() {
        if entry.is_present() {
            tracing::trace!(idx, ?entry);
            present_entries += 1;
        }
    }
    tracing::info!(present_entries);
}

impl PageTable<level::Pml4> {
    pub unsafe fn current(phys_offset: VAddr) -> &'static mut Self {
        let (phys, _) = crate::control_regs::cr3::read();
        let phys = phys.base_address().as_usize();
        let virt = phys_offset + phys;
        tracing::trace!(current_pml4_vaddr = ?virt);
        &mut *(virt.as_ptr::<Self>() as *mut _)
    }
}

impl<S> TranslatePage<S> for PageTable<level::Pml4>
where
    S: Size,
    PageTable<level::Pdpt>: TranslatePage<S>,
{
    fn translate_page(&self, virt: Page<VAddr, S>) -> TranslateResult<PAddr, S> {
        self.next_table(virt.base_address())?.translate_page(virt)
    }
}

impl TranslatePage<Size1Gb> for PageTable<level::Pdpt> {
    fn translate_page(&self, virt: Page<VAddr, Size1Gb>) -> TranslateResult<PAddr, Size1Gb> {
        self[&virt].phys_page()
    }
}

impl TranslatePage<Size2Mb> for PageTable<level::Pdpt> {
    fn translate_page(&self, virt: Page<VAddr, Size2Mb>) -> TranslateResult<PAddr, Size2Mb> {
        self.next_table(virt.base_address())?.translate_page(virt)
    }
}

impl TranslatePage<Size4Kb> for PageTable<level::Pdpt> {
    fn translate_page(&self, virt: Page<VAddr, Size4Kb>) -> TranslateResult<PAddr, Size4Kb> {
        self.next_table(virt.base_address())?.translate_page(virt)
    }
}

impl TranslatePage<Size2Mb> for PageTable<level::Pd> {
    fn translate_page(&self, virt: Page<VAddr, Size2Mb>) -> TranslateResult<PAddr, Size2Mb> {
        self[&virt].phys_page()
    }
}

impl TranslatePage<Size4Kb> for PageTable<level::Pd> {
    fn translate_page(&self, virt: Page<VAddr, Size4Kb>) -> TranslateResult<PAddr, Size4Kb> {
        self.next_table(virt.base_address())?.translate_page(virt)
    }
}

impl TranslatePage<Size4Kb> for PageTable<level::Pt> {
    fn translate_page(&self, virt: Page<VAddr, Size4Kb>) -> TranslateResult<PAddr, Size4Kb> {
        self[&virt].phys_page()
    }
}

impl TranslateAddr for PageTable<level::Pml4> {
    fn translate_addr(&self, virt: VAddr) -> Option<PAddr> {
        let pdpt = self.next_table(virt)?;
        pdpt.translate_addr(virt)
    }
}

impl TranslateAddr for PageTable<level::Pdpt> {
    fn translate_addr(&self, virt: VAddr) -> Option<PAddr> {
        unimplemented!()
    }
}
// this is factored out so we can unit test it using `usize`s without having to
// construct page tables in memory.
fn next_table_addr(my_addr: usize, idx: usize) -> usize {
    (my_addr << 9) | (idx << 12)
}

impl<R: level::Recursive> PageTable<R> {
    fn next_table_ptr(&self, idx: usize) -> Option<NonNull<PageTable<R::Next>>> {
        if !self.entries[idx].is_present() {
            return None;
        }

        let my_addr = self as *const _ as usize;
        let next_addr = next_table_addr(my_addr, idx);
        unsafe {
            let ptr = next_addr as *mut _;
            // we constructed this address & know it won't be null.
            Some(NonNull::new_unchecked(ptr))
        }
    }

    fn next_table<'a>(&'a self, idx: VAddr) -> Option<&'a PageTable<R::Next>> {
        self.next_table_ptr(R::index_of(idx))
            .map(|ptr| unsafe { &*ptr.as_ptr() })
    }

    fn next_table_mut(&mut self, idx: VAddr) -> Option<&mut PageTable<R::Next>> {
        self.next_table_ptr(R::index_of(idx))
            .map(|ptr| unsafe { &mut *ptr.as_ptr() })
    }

    fn create_next_table(
        &mut self,
        idx: VAddr,
        alloc: &mut impl page::Alloc<Size4Kb>,
    ) -> &mut PageTable<R::Next> {
        let span = tracing::trace_span!("create_next_table", ?idx, level = %<R::Next>::NAME);
        let _e = span.enter();

        if self.next_table(idx).is_some() {
            tracing::trace!("next table already exists");
            self.next_table_mut(idx)
                .expect("if next_table().is_some(), the next table exists!")
        } else {
            let entry = &mut self[idx];
            if entry.is_huge() {
                panic!(
                    "cannot create {} table for {:?}: the corresponding entry is huge!\n{:#?}",
                    <R::Next>::NAME,
                    idx,
                    entry
                );
            }

            let frame = match alloc.alloc() {
                Ok(frame) => frame,
                Err(e) => panic!(
                    "cannot create {} table for {:?}: allocation failed!",
                    <R::Next>::NAME,
                    idx,
                ),
            };

            tracing::trace!(?frame, "allocated page table frame");

            let table = PageTable::<R::Next>::new(frame);
            entry
                .set_next_table(table)
                .set_present(true)
                .set_writable(true);
            tracing::trace!("set entry to point at new page table");
            // TODO(eliza): do we need to invlpg???
            unsafe { &mut *table }
        }
    }
}

impl<L: Level> PageTable<L> {
    pub fn zero(&mut self) {
        for e in &mut self.entries[..] {
            *e = Entry::none();
        }
    }

    fn new(frame: Page<PAddr, Size4Kb>) -> *mut Self {
        let this = frame.base_address().as_ptr::<Self>();
        unsafe {
            (*this).zero();
        }
        this
    }
}

impl<L: Level> ops::Index<VAddr> for PageTable<L> {
    type Output = Entry<L>;
    fn index(&self, addr: VAddr) -> &Self::Output {
        &self.entries[L::index_of(addr)]
    }
}

impl<L: Level> ops::IndexMut<VAddr> for PageTable<L> {
    fn index_mut(&mut self, addr: VAddr) -> &mut Self::Output {
        &mut self.entries[L::index_of(addr)]
    }
}

impl<L, S> ops::Index<VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: level::HoldsSize<S>,
    S: Size,
{
    type Output = Entry<L>;
    fn index(&self, pg: VirtPage<S>) -> &Self::Output {
        &self[pg.base_address()]
    }
}

impl<L, S> ops::IndexMut<VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: level::HoldsSize<S>,
    S: Size,
{
    fn index_mut(&mut self, pg: VirtPage<S>) -> &mut Self::Output {
        &mut self[pg.base_address()]
    }
}

impl<'a, L, S> ops::Index<&'a VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: level::HoldsSize<S>,
    S: Size,
{
    type Output = Entry<L>;
    fn index(&self, pg: &'a VirtPage<S>) -> &Self::Output {
        &self[pg.base_address()]
    }
}

impl<'a, L, S> ops::IndexMut<&'a VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: level::HoldsSize<S>,
    S: Size,
{
    fn index_mut(&mut self, pg: &'a VirtPage<S>) -> &mut Self::Output {
        &mut self[pg.base_address()]
    }
}

impl<L> Entry<L> {
    const PRESENT: u64 = 1 << 0;
    const WRITABLE: u64 = 1 << 1;
    const WRITE_THROUGH: u64 = 1 << 3;
    const CACHE_DISABLE: u64 = 1 << 4;
    const ACCESSED: u64 = 1 << 5;
    const DIRTY: u64 = 1 << 6;
    const HUGE: u64 = 1 << 7;
    const GLOBAL: u64 = 1 << 8;
    const NOEXEC: u64 = 1 << 63;

    const ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

    const fn none() -> Self {
        Self {
            entry: 0,
            _level: PhantomData,
        }
    }

    fn set_present(&mut self, present: bool) -> &mut Self {
        if present {
            self.entry |= Self::PRESENT;
        } else {
            self.entry &= !Self::PRESENT;
        }
        self
    }

    fn set_writable(&mut self, writable: bool) -> &mut Self {
        if writable {
            self.entry |= Self::WRITABLE;
        } else {
            self.entry &= !Self::WRITABLE;
        }
        self
    }

    /// Note that this can only be used if the appropriate feature bit is
    /// enabled in the EFER MSR.
    fn set_executable(&mut self, executable: bool) -> &mut Self {
        if executable {
            self.entry &= !Self::NOEXEC;
        } else {
            self.entry |= Self::NOEXEC;
        }
        self
    }

    fn set_huge(&mut self, huge: bool) -> &mut Self {
        if huge {
            self.entry |= Self::HUGE;
        } else {
            self.entry &= !Self::HUGE;
        }
        self
    }

    fn set_phys_addr(&mut self, paddr: PAddr) -> &mut Self {
        let paddr = paddr.as_usize() as u64;
        assert_eq!(paddr & !Self::ADDR_MASK, 0);
        self.entry = (self.entry & !Self::ADDR_MASK) | paddr;
        self
    }

    fn is_present(&self) -> bool {
        self.entry & Self::PRESENT != 0
    }

    fn is_huge(&self) -> bool {
        self.entry & Self::HUGE != 0
    }

    fn is_executable(&self) -> bool {
        self.entry & Self::NOEXEC == 0
    }

    fn is_writable(&self) -> bool {
        self.entry & Self::WRITABLE != 0
    }

    fn phys_addr(&self) -> PAddr {
        PAddr::from_u64(self.entry & Self::ADDR_MASK)
    }
}

impl<L: level::PointsToPage> Entry<L> {
    fn phys_page(&self) -> TranslateResult<PAddr, L::Size> {
        if !self.is_present() {
            return Err(TranslateError::NotMapped);
        }

        if self.is_huge() != L::IS_HUGE {
            return Err(TranslateError::WrongSize(PhantomData));
        }

        Ok(Page::starting_at(self.phys_addr()).expect("page addr must be aligned"))
    }

    fn set_phys_page(&mut self, page: Page<PAddr, L::Size>) -> &mut Self {
        self.set_phys_addr(page.base_address());
        self
    }
}

impl<L: level::Recursive> Entry<L> {
    fn set_next_table(&mut self, table: *mut PageTable<L::Next>) -> &mut Self {
        self.set_phys_addr(PAddr::from_u64(table as u64))
    }
}

impl<L: Level> page::PageFlags for Entry<L> {
    #[inline]
    fn is_writable(&self) -> bool {
        Entry::is_writable(self)
    }
    #[inline]
    fn set_writable(&mut self, writable: bool) -> &mut Self {
        Entry::set_writable(self, writable)
    }

    #[inline]
    fn is_executable(&self) -> bool {
        Entry::is_executable(self)
    }

    #[inline]
    fn set_executable(&mut self, executable: bool) -> &mut Self {
        Entry::set_executable(self, executable)
    }
}

impl<L: Level> fmt::Debug for Entry<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct FmtFlags<'a, L>(&'a Entry<L>);
        impl<'a, L> fmt::Debug for FmtFlags<'a, L> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                macro_rules! write_flags {
                    ($f:expr, $e:expr, $($bit:ident),+) => {
                        let mut wrote_any = false;
                        $(
                            if $e & Entry::<L>::$bit != 0 {
                                write!($f, "{}{}", if wrote_any { " | " } else { "" }, stringify!($bit))?;
                                wrote_any = true;
                            }
                        )+
                    }
                }

                write_flags! {
                    f, self.0.entry,
                    PRESENT,
                    WRITABLE,
                    WRITE_THROUGH,
                    CACHE_DISABLE,
                    ACCESSED,
                    DIRTY,
                    HUGE,
                    GLOBAL
                }
                Ok(())
            }
        }
        f.debug_struct("Entry")
            .field("level", &format_args!("{}", L::NAME))
            .field("addr", &self.phys_addr())
            .field("flags", &FmtFlags(&self))
            .finish()
    }
}

pub mod size {
    use super::Size;

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub enum Size4Kb {}

    impl Size for Size4Kb {
        const SIZE: usize = 4 * 1024;
        const PRETTY_NAME: &'static str = "4KB";
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub enum Size2Mb {}

    impl Size for Size2Mb {
        const SIZE: usize = Size4Kb::SIZE * 512;
        const PRETTY_NAME: &'static str = "2MB";
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub enum Size1Gb {}

    impl Size for Size1Gb {
        const SIZE: usize = Size2Mb::SIZE * 512;

        const PRETTY_NAME: &'static str = "1GB";
    }
}

pub trait Level {
    const NAME: &'static str;
    const SUBLEVELS: usize;

    const INDEX_SHIFT: usize = 12 + (9 * Self::SUBLEVELS);

    fn index_of(v: VAddr) -> usize {
        const INDEX_MASK: usize = 0o777;
        (v.as_usize() >> Self::INDEX_SHIFT) & INDEX_MASK
    }
}

pub mod level {
    use super::{size::*, Level, Size};

    pub trait PointsToPage: Level {
        type Size: Size;
        const IS_HUGE: bool;
    }

    pub trait Recursive: Level {
        type Next: Level;
        const SUBLEVELS: usize = 3;
    }

    pub trait HoldsSize<S: Size>: Level {}

    pub enum Pml4 {}
    pub enum Pdpt {}

    impl Level for Pml4 {
        const SUBLEVELS: usize = 3;
        const NAME: &'static str = "PML4";
    }
    impl HoldsSize<Size1Gb> for Pml4 {}
    impl HoldsSize<Size2Mb> for Pml4 {}
    impl HoldsSize<Size4Kb> for Pml4 {}

    impl Level for Pdpt {
        const SUBLEVELS: usize = 2;
        const NAME: &'static str = "PDPT";
    }
    impl HoldsSize<Size1Gb> for Pdpt {}
    impl HoldsSize<Size2Mb> for Pdpt {}
    impl HoldsSize<Size4Kb> for Pdpt {}

    impl Recursive for Pml4 {
        type Next = Pdpt;
    }

    impl PointsToPage for Pdpt {
        type Size = Size1Gb;
        const IS_HUGE: bool = true;
    }

    impl Recursive for Pdpt {
        type Next = Pd;
    }

    pub enum Pd {}

    impl Level for Pd {
        const NAME: &'static str = "PD";
        const SUBLEVELS: usize = 1;
    }

    impl Recursive for Pd {
        type Next = Pt;
    }

    impl PointsToPage for Pd {
        type Size = Size2Mb;
        const IS_HUGE: bool = true;
    }
    impl HoldsSize<Size2Mb> for Pd {}
    impl HoldsSize<Size4Kb> for Pd {}

    pub enum Pt {}

    impl Level for Pt {
        const NAME: &'static str = "PT";
        const SUBLEVELS: usize = 0;
    }

    impl PointsToPage for Pt {
        type Size = Size4Kb;
        const IS_HUGE: bool = false;
    }
    impl HoldsSize<Size4Kb> for Pt {}
}

pub(crate) mod tlb {
    use crate::control_regs::cr3;
    use crate::VAddr;
    use hal_core::Address;

    pub(crate) unsafe fn flush_all() {
        let (pml4_paddr, flags) = cr3::read();
        cr3::write(pml4_paddr, flags);
    }

    /// Note: this is not available on i386 and earlier.
    // XXX(eliza): can/should this be feature flagged? do we care at all about
    // supporting 80386s from 1985?
    pub(crate) unsafe fn flush_page(addr: VAddr) {
        asm!("invlpg [$0]" :: "r"(addr.as_usize() as u64) : "memory" : "intel")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn next_table_addr_calc() {
        let pml4_addr = 0o177777_777_777_777_777_0000;
        let pml4_idx = 0o111;
        let pdpt_addr = next_table_addr(pml4_addr, pml4_idx);
        assert_eq!(pdpt_addr, 0o177777_777_777_777_111_0000);

        let pdpt_idx = 0o222;
        let pd_addr = next_table_addr(pdpt_addr, pdpt_idx);
        assert_eq!(pd_addr, 0o177777_777_777_111_222_0000);

        let pt_idx = 0o333;
        let pt_addr = next_table_addr(pd_addr, pt_idx);
        assert_eq!(pt_addr, 0o177777_777_111_222_333_0000);
    }
}
