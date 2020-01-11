use crate::{PAddr, VAddr, X64};
use core::{
    fmt,
    marker::PhantomData,
    ops,
    ptr::{self, NonNull},
};
use hal_core::{
    mem::page::{Page, Size, Translate},
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
    _level: PhantomData<fn(L)>,
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

impl<S: Size> Translate<X64, S> for PageTable<level::Pml4> {
    fn translate_page(&self, virt: Page<VAddr, S>) -> Result<Page<PAddr, S>, TranslateError<S>> {
        unimplemented!()
    }
}

impl<R: RecursiveLevel> PageTable<R> {
    fn next_table_ptr(&self, idx: usize) -> Option<NonNull<PageTable<R::Next>>> {
        if !self.entries[idx].is_present() {
            return None;
        }

        let my_addr = self as *const _ as usize;
        let next_addr = (my_addr << 9) | (idx << 12);
        unsafe {
            let ptr = next_addr as *const _;
            // we constructed this address & know it won't be null.
            Some(NonNull::new_unchecked(ptr))
        }
    }

    fn next_table(&self, idx: usize) -> Option<&PageTable<R::Next>> {
        self.next_table_ptr(idx).map(|ptr| unsafe { ptr.as_ref() })
    }

    fn next_table_mut(&mut self, idx: usize) -> Option<&mut PageTable<R::Next>> {
        self.next_table_ptr(idx).map(|ptr| unsafe { ptr.as_mut() })
    }
}

impl<L: Level> ops::Index<VAddr> for PageTable<L> {
    type Output = Entry<L>;
    fn index(&self, addr: VAddr) -> &Self::Output {
        self.entries[L::index_of(addr)]
    }
}

impl<L: Level> ops::IndexMut<VAddr> for PageTable<L> {
    fn index_mut(&mut self, addr: VAddr) -> &mut Self::Output {
        self.entries[L::index_of(addr)]
    }
}

impl<L, S> ops::Index<VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: HoldsSize<S>,
    S: Size,
{
    type Output = Entry<L>;
    fn index(&self, pg: VirtPage<S>) -> &Self::Output {
        &self[pg.base_addr()]
    }
}

impl<L, S> ops::IndexMut<VirtPage<S>> for PageTable<L>
where
    L: Level,
    L: HoldsSize<S>,
    S: Size,
{
    fn index_mut(&mut self, addr: VAddr) -> &mut Self::Output {
        &mut self.entries[pg.base_addr()]
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

    fn is_present(&self) -> bool {
        self.entry & Self::PRESENT != 0
    }
}

impl<L: Level> Entry<L> {
    pub fn phys_addr(&self) -> PAddr {
        PAddr::from_u64(self.entry & L::ADDR_MASK)
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

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Size4K {}

impl Size for Size4K {
    const SIZE: usize = 4 * 1024;
}

pub trait Level {
    const ADDR_MASK: u64;
    const NAME: &'static str;
    const SUBLEVELS: usize;

    fn index_of(v: VAddr) -> usize {
        const INDEX_SHIFT: usize = 12 + (9 * Self::SUBLEVELS);
        const INDEX_MASK: usize = 0o777;
        (v.as_usize() >> INDEX_SHIFT) & INDEX_MASK
    }
}

pub trait HoldsSize<S: hal_core::mem::page::Size>: Level {}

pub trait RecursiveLevel: Level {
    type Next: Level;
}

pub mod level {
    use super::Level;
    pub enum Pml4 {}
    pub enum Pdpt {}

    impl Level for Pml4 {
        const ADDR_MASK: u64 = 0x000fffff_fffff000;
        const NAME: &'static str = "PML4";
        const SUBLEVELS: usize = 3;
    }

    impl Level for Pdpt {
        const ADDR_MASK: u64 = 0xdead;
        const NAME: &'static str = "PDPT";
        const SUBLEVELS: usize = 2;
    }
}
