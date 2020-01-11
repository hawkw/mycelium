pub mod cr3 {
    use crate::{mm::Size4K, PAddr};
    use hal_core::mem::page::Page;

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Flags(u64);

    pub fn read() -> (Page<PAddr, Size4K>, Flags) {
        let val: u64;
        unsafe {
            asm!("mov %cr3, $0" : "=r"(val));
        };
        let addr = PAddr::from_u64(val);
        let pml4_page =
            Page::starting_at(addr).expect("PML4 physical addr not aligned! this is very bad");
        (pml4_page, Flags(val))
    }
}
