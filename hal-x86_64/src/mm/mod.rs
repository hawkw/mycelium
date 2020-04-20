use self::size::*;
use crate::{PAddr, VAddr};
use core::{
    fmt,
    marker::PhantomData,
    ops,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};
use hal_core::{
    mem::page::{
        self, Map, Page, Size, TranslateAddr, TranslateError, TranslatePage, TranslateResult,
    },
    Address,
};

const ENTRIES: usize = 512;

type VirtPage<S> = Page<VAddr, S>;
type PhysPage<S> = Page<PAddr, S>;

pub struct PageCtrl {
    pml4: NonNull<PageTable<level::Pml4>>,
}

#[must_use = "page table updates will not be reflected until the changed pages are flushed from the TLB"]
pub struct Handle<'a, L: level::PointsToPage> {
    page: Page<VAddr, L::Size>,
    entry: &'a mut Entry<L>,
}

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

#[tracing::instrument(level = "info")]
pub fn init_paging(vm_offset: VAddr) {
    VM_OFFSET.store(vm_offset.as_usize(), Ordering::Release);

    tracing::info!("initializing paging...");

    let (pml4_page, flags) = crate::control_regs::cr3::read();
    tracing::debug!(?pml4_page, ?flags);
    tracing::trace!("old PML4:");
    let pml4 = PageTable::<level::Pml4>::current(vm_offset);

    // Log out some details about our starting page table.
    let mut present_entries = 0;
    for (idx, entry) in (&pml4.entries[..]).iter().enumerate() {
        if entry.is_present() {
            tracing::trace!(idx, ?entry);
            present_entries += 1;
        }
    }
    tracing::trace!(present_entries);

    // Create the recursive pagetable entry.
    assert!(
        !pml4.entries[RECURSIVE_INDEX].is_present(),
        "bootloader must not have used entry 511"
    );
    pml4.entries[RECURSIVE_INDEX]
        .set_present(true)
        .set_writable(true)
        .set_phys_addr(pml4_page.base_address());
    tracing::info!("recursive entry created");

    // Forcibly flush the entire TLB by resetting cr3.
    unsafe {
        crate::control_regs::cr3::write(pml4_page, flags);
    }
    tracing::info!("new PML4 set");

    // Log out some details about our starting page table.
    let mut present_entries = 0;
    for (idx, entry) in (&pml4.entries[..]).iter().enumerate() {
        if entry.is_present() {
            tracing::trace!(idx, ?entry);
            present_entries += 1;
        }
    }
    tracing::trace!(present_entries);
}

/// This value should only be set once, early in the kernel boot process before
/// we have access to multiple cores. So, technically, it could be a `static
/// mut`. But, using an atomic is safe, even though it's not strictly necessary.
static VM_OFFSET: AtomicUsize = AtomicUsize::new(core::usize::MAX);

impl PageTable<level::Pml4> {
    fn current(vm_offset: VAddr) -> &'static mut Self {
        let (phys, _) = crate::control_regs::cr3::read();
        unsafe { Self::from_pml4_page(vm_offset, phys) }
    }
    unsafe fn from_pml4_page(vm_offset: VAddr, page: Page<PAddr, Size4Kb>) -> &'static mut Self {
        let pml4_paddr = page.base_address();
        tracing::trace!(?pml4_paddr, ?vm_offset);

        let virt = vm_offset + VAddr::from_usize(pml4_paddr.as_usize());
        tracing::debug!(current_pml4_vaddr = ?virt);
        &mut *(virt.as_ptr::<Self>() as *mut _)
    }
}

impl<'mapper, A> Map<'mapper, Size4Kb, A> for PageCtrl
where
    A: page::Alloc<Size4Kb>,
{
    type Handle = Handle<'mapper, level::Pt>;

    unsafe fn map_page(
        &'mapper mut self,
        virt: Page<VAddr, Size4Kb>,
        phys: Page<PAddr, Size4Kb>,
        frame_alloc: &mut A,
    ) -> Self::Handle {
        // XXX(eliza): most of this fn is *internally* safe and should be
        // factored out into a safe function...
        let span = tracing::debug_span!("map_page", ?virt, ?phys);
        let _e = span.enter();
        let pml4 = self.pml4.as_mut();

        let vaddr = virt.base_address();
        tracing::trace!(?vaddr);

        let page_table = pml4
            .create_next_table(virt, frame_alloc)
            .create_next_table(virt, frame_alloc)
            .create_next_table(virt, frame_alloc);
        tracing::debug!(?page_table);

        let entry = &mut page_table[virt];
        assert!(
            !entry.is_present(),
            "mapped page table entry already in use"
        );
        assert!(!entry.is_huge(), "huge bit should not be set for 4KB entry");
        let entry = entry.set_phys_page(phys).set_present(true);
        Handle { entry, page: virt }
    }

    fn flags_mut(&'mapper mut self, _virt: Page<VAddr, Size4Kb>) -> Self::Handle {
        unimplemented!()
    }

    unsafe fn unmap(&'mapper mut self, _virt: Page<VAddr, Size4Kb>) -> Page<PAddr, Size4Kb> {
        unimplemented!()
    }
}

impl<S> TranslatePage<S> for PageCtrl
where
    S: Size,
    PageTable<level::Pml4>: TranslatePage<S>,
{
    fn translate_page(&self, virt: Page<VAddr, S>) -> TranslateResult<PAddr, S> {
        unsafe { self.pml4.as_ref() }.translate_page(virt)
    }
}

impl PageCtrl {
    pub fn current() -> Self {
        let vm_offset = VM_OFFSET.load(Ordering::Acquire);
        assert_ne!(
            vm_offset,
            core::usize::MAX,
            "`init_paging` must be called before calling `PageTable::current`!"
        );
        let vm_offset = VAddr::from_usize(vm_offset);
        let pml4 = PageTable::current(vm_offset);
        Self {
            pml4: NonNull::from(pml4),
        }
    }
}

impl<S> TranslatePage<S> for PageTable<level::Pml4>
where
    S: Size,
    PageTable<level::Pdpt>: TranslatePage<S>,
{
    fn translate_page(&self, virt: Page<VAddr, S>) -> TranslateResult<PAddr, S> {
        self.next_table(virt)
            .ok_or(TranslateError::NotMapped)?
            .translate_page(virt)
    }
}

impl TranslatePage<Size1Gb> for PageTable<level::Pdpt> {
    fn translate_page(&self, virt: Page<VAddr, Size1Gb>) -> TranslateResult<PAddr, Size1Gb> {
        self[&virt].phys_page()
    }
}

impl TranslatePage<Size2Mb> for PageTable<level::Pdpt> {
    fn translate_page(&self, virt: Page<VAddr, Size2Mb>) -> TranslateResult<PAddr, Size2Mb> {
        self.next_table(virt)
            .ok_or(TranslateError::NotMapped)?
            .translate_page(virt)
    }
}

impl TranslatePage<Size4Kb> for PageTable<level::Pdpt> {
    fn translate_page(&self, virt: Page<VAddr, Size4Kb>) -> TranslateResult<PAddr, Size4Kb> {
        self.next_table(virt)
            .ok_or(TranslateError::NotMapped)?
            .translate_page(virt)
    }
}

impl TranslatePage<Size2Mb> for PageTable<level::Pd> {
    fn translate_page(&self, virt: Page<VAddr, Size2Mb>) -> TranslateResult<PAddr, Size2Mb> {
        self[&virt].phys_page()
    }
}

impl TranslatePage<Size4Kb> for PageTable<level::Pd> {
    fn translate_page(&self, virt: Page<VAddr, Size4Kb>) -> TranslateResult<PAddr, Size4Kb> {
        self.next_table(virt)
            .ok_or(TranslateError::NotMapped)?
            .translate_page(virt)
    }
}

impl TranslatePage<Size4Kb> for PageTable<level::Pt> {
    fn translate_page(&self, virt: Page<VAddr, Size4Kb>) -> TranslateResult<PAddr, Size4Kb> {
        self[&virt].phys_page()
    }
}

impl TranslateAddr for PageTable<level::Pml4> {
    fn translate_addr(&self, _virt: VAddr) -> Option<PAddr> {
        unimplemented!()
    }
}

impl TranslateAddr for PageTable<level::Pdpt> {
    fn translate_addr(&self, _virt: VAddr) -> Option<PAddr> {
        unimplemented!()
    }
}

impl<R: level::Recursive> PageTable<R> {
    #[inline(always)]
    fn next_table_ptr<S: Size>(&self, idx: VirtPage<S>) -> Option<NonNull<PageTable<R::Next>>> {
        let span = tracing::debug_span!(
            "next_table_ptr",
            ?idx,
            self.level = %R::NAME,
            next.level = %<R::Next>::NAME,
            self.addr = ?&self as *const _,
        );
        let _e = span.enter();
        let entry = &self[idx];
        tracing::trace!(?entry);
        if !entry.is_present() {
            tracing::debug!("entry not present!");
            return None;
        }
        // XXX(eliza): should we have a different return type to distinguish
        // between "the entry is huge, so you stop the page table walk" and "the entry is not
        // present"? we definitely should...
        if entry.is_huge() {
            tracing::debug!("page is hudge!");
            return None;
        }
        let vaddr = R::Next::table_addr(idx.base_address());
        tracing::trace!(next.addr = ?vaddr, "found next table virtual address");
        // XXX(eliza): this _probably_ could be be a `new_unchecked`...if, after
        // all this, the next table address is null...we're probably pretty fucked!
        NonNull::new(vaddr.as_ptr())
    }

    #[inline]
    // #[tracing::instrument(skip(self))]
    fn next_table<S: Size>(&self, idx: VirtPage<S>) -> Option<&PageTable<R::Next>> {
        Some(unsafe { &*self.next_table_ptr(idx)?.as_ptr() })
    }

    #[inline]
    #[tracing::instrument(skip(self))]
    fn next_table_mut<S: Size>(&mut self, idx: VirtPage<S>) -> Option<&mut PageTable<R::Next>> {
        Some(unsafe { &mut *self.next_table_ptr(idx)?.as_ptr() })
    }

    fn create_next_table<S: Size>(
        &mut self,
        idx: VirtPage<S>,
        alloc: &mut impl page::Alloc<Size4Kb>,
    ) -> &mut PageTable<R::Next> {
        let span = tracing::trace_span!("create_next_table", ?idx, self.level = %R::NAME, next.level = %<R::Next>::NAME);
        let _e = span.enter();
        for (idx, entry) in (&self.entries[..]).iter().enumerate() {
            if entry.is_present() {
                tracing::trace!(idx, ?entry);
            }
        }
        if self.next_table(idx).is_some() {
            tracing::trace!("next table already exists");
            return self
                .next_table_mut(idx)
                .expect("if next_table().is_some(), the next table exists!");
        }

        tracing::trace!("no next table exists");
        let entry = &mut self[idx];
        if entry.is_huge() {
            panic!(
                "cannot create {} table for {:?}: the corresponding entry is huge!\n{:#?}",
                <R::Next>::NAME,
                idx,
                entry
            );
        }

        tracing::trace!("trying to allocate page table frame...");
        let frame = match alloc.alloc() {
            Ok(frame) => frame,
            Err(_) => panic!(
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
        unsafe { &mut *table }
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

impl<L, S> ops::Index<VirtPage<S>> for PageTable<L>
where
    L: Level,
    S: Size,
{
    type Output = Entry<L>;
    fn index(&self, page: VirtPage<S>) -> &Self::Output {
        &self.entries[L::index_of(page.base_address())]
    }
}

impl<L, S> ops::IndexMut<VirtPage<S>> for PageTable<L>
where
    L: Level,
    S: Size,
{
    fn index_mut(&mut self, page: VirtPage<S>) -> &mut Self::Output {
        &mut self.entries[L::index_of(page.base_address())]
    }
}

impl<'a, L, S> ops::Index<&'a VirtPage<S>> for PageTable<L>
where
    L: Level,
    S: Size,
{
    type Output = Entry<L>;
    fn index(&self, pg: &'a VirtPage<S>) -> &Self::Output {
        &self[pg.clone()]
    }
}

impl<'a, L, S> ops::IndexMut<&'a VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: level::HoldsSize<S>,
    S: Size,
{
    fn index_mut(&mut self, pg: &'a VirtPage<S>) -> &mut Self::Output {
        &mut self[pg.clone()]
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

    #[allow(dead_code)] // we'll need this later
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

impl<'a, L: level::PointsToPage> page::PageFlags<L::Size> for Handle<'a, L> {
    #[inline]
    fn is_writable(&self) -> bool {
        self.entry.is_writable()
    }

    #[inline]
    unsafe fn set_writable(&mut self, writable: bool) -> &mut Self {
        self.entry.set_writable(writable);
        self
    }

    #[inline]
    fn is_executable(&self) -> bool {
        self.entry.is_executable()
    }

    #[inline]
    unsafe fn set_executable(&mut self, executable: bool) -> &mut Self {
        self.entry.set_executable(executable);
        self
    }

    #[inline]
    fn is_present(&self) -> bool {
        self.entry.is_present()
    }

    #[inline]
    unsafe fn set_present(&mut self, present: bool) -> &mut Self {
        self.entry.set_present(present);
        self
    }

    fn commit(self) -> Page<VAddr, L::Size> {
        unsafe {
            tlb::flush_page(self.page.base_address());
        }
        self.page
    }
}

impl<L: Level> fmt::Debug for Entry<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct FmtFlags<'a, L>(&'a Entry<L>);
        impl<'a, L> fmt::Debug for FmtFlags<'a, L> {
            #[inline(always)]
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                macro_rules! write_flags {
                    ($f:expr, $e:expr, $($bit:ident),+) => {
                        let mut wrote_any = false;
                        $(
                            if $e & Entry::<L>::$bit != 0 {
                                write!($f, "{}{}", if wrote_any { " | " } else { "" }, stringify!($bit))?;
                                // the last one is not used but we can't tell the macro that easily.
                                #[allow(unused_assignments)]
                                {
                                    wrote_any = true;
                                }
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

const RECURSIVE_INDEX: usize = 0o777;
const SIGN: usize = 0o177777 << 48;
pub trait Level {
    const NAME: &'static str;
    const SUBLEVELS: usize;

    const INDEX_SHIFT: usize = 12 + (9 * Self::SUBLEVELS);

    fn index_of(v: VAddr) -> usize {
        (v.as_usize() >> Self::INDEX_SHIFT) & RECURSIVE_INDEX
    }

    fn table_addr(v: VAddr) -> VAddr;
}

impl<L: Level> fmt::Debug for PageTable<L> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PageTable")
            .field("level", &format_args!("{}", L::NAME))
            .field("addr", &format_args!("{:p}", self))
            .finish()
    }
}

pub mod level {
    use super::{size::*, Level, Size, RECURSIVE_INDEX, SIGN};
    use crate::VAddr;
    use hal_core::Address;

    pub trait PointsToPage: Level {
        type Size: Size;
        const IS_HUGE: bool;
    }

    pub trait Recursive: Level {
        type Next: Level;
    }

    pub trait HoldsSize<S: Size>: Level {}

    pub enum Pml4 {}
    pub enum Pdpt {}

    impl Level for Pml4 {
        const SUBLEVELS: usize = 3;
        const NAME: &'static str = "PML4";
        fn table_addr(v: VAddr) -> VAddr {
            let addr = SIGN
                | (RECURSIVE_INDEX << 39)
                | (RECURSIVE_INDEX << 30)
                | (RECURSIVE_INDEX << 21)
                | (RECURSIVE_INDEX << 12);
            tracing::trace!(?v);
            VAddr::from_usize(addr)
        }
    }

    impl HoldsSize<Size1Gb> for Pml4 {}
    impl HoldsSize<Size2Mb> for Pml4 {}
    impl HoldsSize<Size4Kb> for Pml4 {}

    impl Level for Pdpt {
        const SUBLEVELS: usize = 2;
        const NAME: &'static str = "PDPT";
        fn table_addr(v: VAddr) -> VAddr {
            let pml4_idx = Pml4::index_of(v);
            let addr = SIGN
                | (RECURSIVE_INDEX << 39)
                | (RECURSIVE_INDEX << 30)
                | (RECURSIVE_INDEX << 21)
                | (pml4_idx << 12);
            tracing::trace!(?v, ?pml4_idx);
            VAddr::from_usize(addr)
        }
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
        fn table_addr(v: VAddr) -> VAddr {
            let pml4_idx = Pml4::index_of(v);
            let pdpt_idx = Pdpt::index_of(v);
            let addr = SIGN
                | (RECURSIVE_INDEX << 39)
                | (RECURSIVE_INDEX << 30)
                | (pml4_idx << 21)
                | (pdpt_idx << 12);
            tracing::trace!(?v, ?pml4_idx, ?pdpt_idx);
            VAddr::from_usize(addr)
        }
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

        fn table_addr(v: VAddr) -> VAddr {
            let pml4_idx = Pml4::index_of(v);
            let pdpt_idx = Pdpt::index_of(v);
            let pd_idx = Pd::index_of(v);
            let addr = SIGN
                | (RECURSIVE_INDEX << 39)
                | (pml4_idx << 30)
                | (pdpt_idx << 21)
                | (pd_idx << 12);
            tracing::trace!(?v, ?pml4_idx, ?pdpt_idx, ?pd_idx);
            VAddr::from_usize(addr)
        }
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

    #[allow(dead_code)] // we'll need this later
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

mycelium_util::decl_test! {
    fn basic_map() -> Result<(), ()> {
        use hal_core::mem::page::PageFlags;
        let mut ctrl = PageCtrl::current();
        // We shouldn't need to allocate page frames for this test.
        let mut frame_alloc = page::EmptyAlloc::default();

        let frame = Page::containing(PAddr::from_usize(0xb8000));
        let page = Page::containing(VAddr::from_usize(0));

        let page = unsafe {
            let mut flags = ctrl.map_page(page, frame, &mut frame_alloc);
            flags.set_writable(true);
            flags.commit()
        };
        tracing::info!(?page, "page mapped!");

        let page_ptr = page.base_address().as_ptr::<u64>();
        unsafe { page_ptr.offset(400).write_volatile(0x_f021_f077_f065_f04e)};

        tracing::info!("wow, it didn't fault");

        Ok(())
    }
}

mycelium_util::decl_test! {
    fn identity_mapped_pages_are_reasonable() -> Result<(), ()> {
        let ctrl = PageCtrl::current();

        let page = VirtPage::<Size4Kb>::containing(VAddr::from_usize(0xb8000));

        let frame = ctrl.translate_page(page).expect("translate");
        let actual_frame = PhysPage::<Size4Kb>::containing(PAddr::from_usize(0xb8000));
        tracing::info!(?page, ?frame, "translated");
        assert_eq!(frame, actual_frame, "identity mapped address should translate to itself");
        Ok(())
    }
}
