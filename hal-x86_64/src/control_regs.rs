pub mod cr3 {
    use crate::{mm::size::Size4Kb, PAddr};
    use hal_core::mem::page::Page;

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Flags(u64);

    pub fn read() -> (Page<PAddr, Size4Kb>, Flags) {
        let val: u64;
        unsafe {
            asm!("mov %cr3, $0" : "=r"(val));
        };
        let addr = PAddr::from_u64(val);
        let pml4_page =
            Page::starting_at(addr).expect("PML4 physical addr not aligned! this is very bad");
        (pml4_page, Flags(val))
    }

    pub unsafe fn write(pml4: Page<PAddr, Size4Kb>, flags: Flags) {
        let addr = pml4.base_address().as_usize() as u64;
        let val = addr | flags.0;
        asm!("mov $0, %cr3" :: "r"(val) : "memory");
    }
}
