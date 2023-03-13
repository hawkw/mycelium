use core::arch::asm;
use hal_core::VAddr;
use mycelium_util::bits::bitfield;

pub mod cr3 {
    use super::*;
    use crate::{mm::size::Size4Kb, PAddr};
    use hal_core::mem::page::Page;

    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Flags(u64);

    pub fn read() -> (Page<PAddr, Size4Kb>, Flags) {
        let val: u64;
        unsafe {
            asm!("mov {0}, cr3", out(reg) val, options(readonly));
        };
        let addr = PAddr::from_u64(val);
        tracing::trace!(rax = ?addr, "mov rax, cr3");
        let pml4_page = Page::starting_at_fixed(addr)
            .expect("PML4 physical addr not aligned! this is very bad");
        (pml4_page, Flags(val))
    }

    /// # Safety
    ///
    /// Writing cr3 can break pretty much everything.
    pub unsafe fn write(pml4: Page<PAddr, Size4Kb>, flags: Flags) {
        let addr = pml4.base_addr().as_usize() as u64;
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

bitfield! {
    /// Control Register 0
    #[derive(Eq, PartialEq)]
    pub struct Cr0<u64> {
        /// Protected Mode Enable (`PE`)
        ///
        /// Enables protected mode.
        pub const PROTECTED_MODE_ENABLE: bool;
        /// Monitor Coprocessor (`MP`).
        ///
        /// Enables monitoring of the coprocessor, typically for x87 instructions.
        ///
        /// Controls (together with the [`TASK_SWITCHED`] flag) whether a `WAIT`
        /// or `FWAIT` instruction should cause an `#NE` exception.
        pub const MONITOR_COPROCESSOR: bool;
        /// x87 FPU Emulation (`EM`).
        ///
        /// Force all x87 and MMX instructions to cause an `#NE` exception.
        pub const EMULATE_COPROCESSOR: bool;
        /// Task Switched (`TS`).
        ///
        /// Set to 1 on hardware task switches. This allows lazily saving x87,
        /// MMX, and SSE state on hardware context switches.
        pub const TASK_SWITCHED: bool;
        /// Extension Type (`ET`).
        ///
        /// Indicates support of 387DX math coprocessor instructions.
        ///
        /// Always set on all recent x86 processors, cannot be cleared.
        pub const EXTENSION_TYPE: bool;
        /// Numeric Error (`NE`).
        ///
        /// Enables native error reporting for x87 FPU errors.
        pub const NUMERIC_ERROR: bool;
        const RESERVED_0 = 11;
        /// Write Protect (`WP`).
        ///
        /// Enables write protection for ring 0 pages.
        pub const WRITE_PROTECT: bool;
        const RESERVED_1 = 1;
        /// Alignment Mask (`AM`).
        ///
        /// Enables user-mode alignment checking if the `ALIGNMENT_CHECK` bit in
        /// `RFLAGS` is also set.
        pub const ALIGNMENT_MASK: bool;
        const RESERVED_2 = 11;
        /// Not Write Through (`NW`).
        pub const NOT_WRITE_THROUGH: bool;
        /// Cache Disable (`NW`).
        pub const CACHE_DISABLE: bool;
        /// Paging Enabled (`PG`).
        ///
        /// Enables paging, if [`PROTECTED_MODE_ENABLE`] is also set.
        pub const PAGING_ENABLE: bool;
    }
}

impl Cr0 {
    #[must_use]
    pub fn read() -> Self {
        let bits: u64;
        unsafe {
            asm!("mov {0}, cr0", out(reg) bits, options(readonly));
        };
        Self::from_bits(bits)
    }

    /// Write a value to `CR0`.
    ///
    /// This function preserves the value of all reserved bits in `CR0`.
    ///
    /// # Safety
    ///
    /// Writing to `CR0` can do stuff.
    pub unsafe fn write(value: Self) {
        Self::update(|current| {
            value
                .with(Self::RESERVED_0, current.get(Self::RESERVED_0))
                .with(Self::RESERVED_1, current.get(Self::RESERVED_1))
                .with(Self::RESERVED_2, current.get(Self::RESERVED_2))
        })
    }

    /// # Safety
    ///
    /// Writing to `CR4` can do stuff.
    pub unsafe fn update(f: impl FnOnce(Self) -> Self) {
        let curr = Self::read();
        let value = f(curr);
        tracing::trace!("mov cr0, {value:?}");
        asm!("mov cr0, {0}", in(reg) value.bits());
    }
}

bitfield! {
    /// Control Register 4
    #[derive(Eq, PartialEq)]
    pub struct Cr4<u64> {
        /// Virtual 8086 Mode Extensions (`VME`).
        pub const VIRTUAL_8086_MODE_EXTENSIONS: bool;
        /// Protected-Mode Virtual Interrupts (`PVI`).
        pub const PROTECTED_MODE_VIRTUAL_INTERRUPTS: bool;
        /// Time Stamp Disable (`TSD`).
        pub const TIME_STAMP_DISABLE: bool;
        /// Debugging Extensions (`DE`).
        pub const DEBUGGING_EXTENSIONS: bool;
        /// Page Size Extension (`PSE`).
        ///
        /// This enables the use of 4MB physical pages; always ignored in long mode.
        pub const PAGE_SIZE_EXTENSION: bool;
        /// Physical Address Extension (`PAE`).
        ///
        /// Enables the use of 2MB physical pages. Required in long mode.
        pub const PHYSICAL_ADDRESS_EXTENSION: bool;
        /// Machine Check Exception Enable (`MCE`).
        pub const MACHINE_CHECK_EXCEPTION: bool;
        /// Page Global Enable (`PGE`).
        ///
        /// Allows marking pages as global.
        pub const PAGE_GLOBAL_ENABLE: bool;
        /// Performance-Monitoring Counter Enable (`PCE`).
        ///
        /// Allows the
        pub const PERFORMANCE_MONITOR_COUNTER: bool;
        /// Operating System `FXSAVE`/`FXRSTOR` Support
        ///
        /// If enabled, the `FXSAVE` and `FXRSTOR` instructions are
        /// available in both 64-bit and compatibility mode.
        pub const OSFXSR: bool;
        /// Operating System Support for Unmasked Floating-Point Exceptions
        pub const OSXMMEXCPT: bool;
        /// User-Mode Instruction Prevention (`UMIP`).
        ///
        /// If set, `SGDT`, `SIDT`, `SLDT`, `SMSW`, and `STR`, instructions
        /// will result in a general protection fault (`#GP`) when in ring >
        /// 0.
        pub const USER_MODE_INSTRUCTION_PREVENTION: bool;
        /// Virtual Machine Extensions (VMX) Enable
        ///
        /// **Note**: this extension is INTEL ONLY.
        pub const VIRTUAL_MACHINE_EXTENSIONS: bool;
        /// Safer Mode Extensions (SMX) Enable
        ///
        /// **Note**: this extension is INTEL ONLY.
        pub const SAFER_MODE_EXTENSIONS: bool;
        /// FSBASE/GSBASE Enable.
        ///
        /// Enables the instructions `RDFSBASE`,`RDGSBASE`, `WRFSBASE`, and `WRGSBASE`.
        pub const FSGSBASE: bool;
        /// PCID Enable (`PCIDE`).
        pub const PCID_ENABLE: bool;
        /// OS Support for `XSAVE` and Processor Extended States Enable
        pub const OSXSAVE: bool;
        /// Supervisor Mode Execution Protection Enable (`SMEP`).
        pub const SUPERVISOR_EXECUTION_PROTECTION: bool;
        /// Supervisor Mode Access Prevention Enable (`SMAP)
        pub const SUPERVISOR_ACCESS_PREVENTION: bool;
        /// Protection Key (Used) Enable (`PKE`).
        ///
        /// Enables protection keys for user-mode pages.
        pub const PROTECTION_KEY_USER: bool;
        /// Control-flow Enforcement Technology Enable (`CET`).
        pub const CONTROL_FLOW_ENFORCEMENT: bool;
        /// Protection Key (Supervisor) Enable (`PKS`).
        ///
        /// Enables protection keys for user-mode pages.
        pub const PROTECTION_KEY_SUPERVISOR: bool;
    }
}

impl Cr4 {
    #[must_use]
    pub fn read() -> Self {
        let bits: u64;
        unsafe {
            asm!("mov {0}, cr4", out(reg) bits, options(readonly));
        };
        Self::from_bits(bits)
    }

    /// # Safety
    ///
    /// Writing to `CR4` can do stuff.
    pub unsafe fn write(value: Self) {
        tracing::trace!("mov cr4, {:?}", value);
        asm!("mov cr4, {0}", in(reg) value.bits());
    }

    /// # Safety
    ///
    /// Writing to `CR4` can do stuff.
    pub unsafe fn update(f: impl FnOnce(Self) -> Self) {
        let curr = Self::read();
        Self::write(f(curr));
    }
}

/// Control Register 2 (CR2) contains the Page Fault Linear Address (PFLA).
pub struct Cr2;

impl Cr2 {
    /// Returns the 32-bit Page Fault Linear Address (PFLA) stored in CR2.
    pub fn read() -> VAddr {
        let addr: u64;
        unsafe {
            asm!("mov {0}, cr2", out(reg) addr, options(readonly));
        };
        VAddr::from_u64(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cr4_valid() {
        Cr4::assert_valid();
    }

    #[test]
    fn cr0_valid() {
        Cr0::assert_valid();
    }
}
