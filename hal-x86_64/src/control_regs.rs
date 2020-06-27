pub mod cr3 {
    use crate::{mm::size::Size4Kb, PAddr};
    use hal_core::{mem::page::Page, Address};

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Flags(u64);

    pub fn read() -> (Page<PAddr, Size4Kb>, Flags) {
        let val: u64;
        unsafe {
            asm!("mov {0}, cr3", out(reg) val, options(readonly));
        };
        let addr = PAddr::from_u64(val);
        let pml4_page = Page::starting_at_fixed(addr)
            .expect("PML4 physical addr not aligned! this is very bad");
        (pml4_page, Flags(val))
    }

    pub unsafe fn write(pml4: Page<PAddr, Size4Kb>, flags: Flags) {
        let addr = pml4.base_address().as_usize() as u64;
        let val = addr | flags.0;
        asm!("mov cr3, {0}", in(reg) val);
    }

    impl core::fmt::Debug for Flags {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_tuple("cr3::Flags")
                .field(&format_args!("{:#b}", self.0))
                .finish()
        }
    }
}
