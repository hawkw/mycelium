use crate::{
    control_regs::{Cr0, Cr4},
    cpu::{
        self,
        msr::{Efer, Msr},
        topology::{self, Processor},
        Ring,
    },
    interrupt::apic::local::{IpiKind, IpiTarget, LocalApic},
    mm::PhysPage,
    segment,
};
use core::{
    arch::global_asm,
    sync::atomic::{AtomicU64, Ordering},
};
use hal_core::PAddr;
use mycelium_util::bits;

impl Processor {
    #[tracing::instrument(name = "bringup_ap", skip(bsp_lapic), err(Display))]
    pub fn bringup_ap(&mut self, bsp_lapic: &LocalApic) -> Result<(), &'static str> {
        tracing::info!("bringing up application processor...");

        // TODO(eliza): check that this is only called by the BSP
        let trampoline_page = PhysPage::starting_at_fixed(AP_TRAMPOLINE_ADDR).map_err(|error| {
            tracing::error!(addr = ?AP_TRAMPOLINE_ADDR, %error, "AP trampoline address invalid!");
            "AP trampoline address invalid"
        })?;

        if self.is_boot_processor {
            return Err(
                "called `bringup_ap` on the BSP! what are you doing and how did this happen?",
            );
        }

        if self.state != topology::State::Idle {
            return Err("AP is not idle");
        }

        tracing::info!("sending INIT IPI to AP {}...", self.lapic_id);
        bsp_lapic
            .send_ipi(
                IpiTarget::ApicId(self.lapic_id as u8),
                IpiKind::Init { assert: true },
            )
            // TODO(eliza): do nice error contexts some day...
            .map_err(|_| "failed to send INIT IPI")?;

        tracing::info!(?trampoline_page, "sending SIPI to AP {}...", self.lapic_id);

        // TODO(eliza): ensure trampoline page is mapped nicely.
        unsafe {
            AP_SPINLOCK.store(0, Ordering::SeqCst);
        };

        bsp_lapic
            .send_ipi(
                IpiTarget::ApicId(self.lapic_id as u8),
                IpiKind::Startup(trampoline_page),
            )
            .map_err(|_| "failed to send SIPI")?;

        // TODO(eliza): spin waiting for AP to start...
        tracing::info!("waiting for AP to start...");
        while unsafe { AP_SPINLOCK.load(Ordering::SeqCst) == 0 } {
            // spin
            tracing::trace!("waiting...");
            core::hint::spin_loop();
        }

        self.state = topology::State::Running;
        // TODO(eliza): AP should call init processor on itself...
        // ap.init_processor(gdt)

        Ok(())
    }
}

const AP_TRAMPOLINE_ADDR: PAddr = match PAddr::from_usize_checked(0x8000) {
    Ok(addr) => addr,
    Err(_) => panic!("invalid AP trampoline address!"),
};

extern "C" {
    #[link_name = "ap_spinlock"]
    static AP_SPINLOCK: AtomicU64;
}

global_asm! {
    // /!\ EXTREMELY MESSED UP HACK: stick this in the `.boot-first-stage`
    // section that's defined by the `bootloader` crate's linker script, so that
    // it gets linked into 16-bit memory. we don't control the linker script, so
    // we can't define our own section and stick it in the right place, but we
    // can piggyback off of `bootloader`'s linker script.
    //
    // OBVIOUSLY THIS WILL CRASH AND BURN IF YOU ARENT LINKING WITH `bootloader`
    // BUT WHATEVER LOL THATS NOT MY PROBLEM,,,
    ".section .boot-first-stage, \"wx\"",
    ".code16",
    ".org {trampoline_addr}",
    ".align 4096",
    ".global ap_trampoline",
    ".global ap_trampoline_end",
    ".global ap_spinlock",
    "ap_trampoline:",
    "   jmp ap_start",
    "   .nops 8",
    "ap_spinlock: .quad 0",
    "ap_pml4: .quad 0",

    "ap_start:",
    "   cli",

     // zero segment registers
    "   xor %ax, %ax",
    "   mov %ax, %ds",
    "   mov %ax, %es",
    "   mov %ax, %ss",

    // initialize stack pointer to an invalid (null) value
    "   mov $0x0, %sp",

    // setup page table
    "mov (ap_pml4), %eax ",
    "mov (%eax), %edi",
    "mov %edi, %cr3",

    // init FPU
    "   fninit",

    // load 32-bit GDT
    "   lgdt (gdt32_ptr)",

    // set CR4 flags
    "   mov %cr4, %eax",
    "   or {cr4flags}, %eax",
    "   mov %eax, %cr4",

    // enable long mode in EFER
    "   mov {efer_num}, %ecx",
    "   rdmsr",
    "   or {efer_bits}, %eax",
    "   wrmsr",

    // set CR0 flags to enable paging and write protection
    "   mov %cr0, %ebx",
    "   or {cr0flags}, %ebx",
    "   mov %ebx, %cr0",

    // far jump to enable Long Mode and load CS with 64 bit segment
    "   jmp $gdt32_kernel_code, $ap_long_mode",
    // 32-bit GDT
    ".align 16",
    "gdt32:",
    "   .long 0, 0",
    "gdt32_kernel_code:",
    "   .quad {gdt32_code}", // code segment
    "gdt32_kernel_data:",
    "   .quad {gdt32_data}", // data segment
    "   .long 0x00000068, 0x00CF8900", // TSS
    "gdt32_ptr:",
    "   .word gdt32_ptr - gdt32 - 1", // size
    "   .word gdt32", // offset
    "ap_trampoline_end:",
    ".code64",
    "ap_long_mode:",
    "   mov %rax, %ds",
    "   mov %rax, %es",
    "   mov %rax, %fs",
    "   mov %rax, %fs",
    "   mov %rax, %gs",
    "   mov %rax, %ss",

    // set spinlock ready
    // "   movq $1, (ap_spinlock)",
    // TODO(eliza): setup ap stack
    trampoline_addr = const AP_TRAMPOLINE_ADDR.as_usize(),
    cr4flags = const AP_CR4,
    cr0flags = const AP_CR0,
    efer_num = const Msr::ia32_efer().num,
    efer_bits = const EFER_LONG_MODE,
    gdt32_code = const segment::Descriptor::code_32()
        .with_ring(Ring::Ring0).bits(),
    gdt32_data = const segment::Descriptor::data_flat_16()
        .bits(),
    // spinlock_ready = const 1,
    options(att_syntax)
}

/// Initial CR4 flags to set for an application processor.
const AP_CR4: u32 = bits::Pack64::pack_in(0)
    .set_all(&Cr4::PAGE_SIZE_EXTENSION)
    .set_all(&Cr4::PHYSICAL_ADDRESS_EXTENSION)
    .set_all(&Cr4::PAGE_GLOBAL_ENABLE)
    .set_all(&Cr4::OSFXSR)
    .bits() as u32;

/// Initial CR0 flags to set for an application processor.
const AP_CR0: u32 = bits::Pack64::pack_in(0)
    .set_all(&Cr0::PROTECTED_MODE_ENABLE)
    .set_all(&Cr0::PAGING_ENABLE)
    .set_all(&Cr0::WRITE_PROTECT)
    .bits() as u32;

/// EFER bits to enable long mode
const EFER_LONG_MODE: u32 = bits::Pack64::pack_in(0)
    .set_all(&Efer::LONG_MODE_ENABLE)
    .set_all(&Efer::NO_EXECUTE_ENABLE)
    .bits() as u32;
