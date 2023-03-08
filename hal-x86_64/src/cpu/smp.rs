use crate::{
    control_regs::{Cr0, Cr4},
    cpu::{
        msr::{Efer, Msr},
        topology::Topology,
        Ring,
    },
    segment,
};
use core::arch::global_asm;
use mycelium_util::bits;

#[tracing::instrument(name = "smp::bringup", skip(topology))]
pub fn bringup(topology: &Topology) -> Result<(), &'static str> {
    unsafe {
        tracing::info!(
            "AP trampoline: {:p} .. {:p}",
            &AP_TRAMPOLINE_START,
            &AP_TRAMPOLINE_END
        );
        assert_eq!(
            &AP_TRAMPOLINE_START as *const _ as usize,
            AP_TRAMPOLINE_ADDR
        );
    }

    Ok(())
}

extern "C" {
    #[link_name = "ap_trampoline"]
    static AP_TRAMPOLINE_START: u8;
    #[link_name = "ap_trampoline_end"]
    static AP_TRAMPOLINE_END: u8;
}

const AP_TRAMPOLINE_ADDR: usize = 0x8000;
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
    "ap_trampoline:",
    "   jmp short ap_start",
    "   .nops 8",
    "ap_spinlock: .quad 0",
    "ap_pml4: .quad 0",

    "ap_start:",
    "   cli",

     // zero segment registers
    "   xor ax, ax",
    "   mov ds, ax",
    "   mov es, ax",
    "   mov ss, ax",

    // initialize stack pointer to an invalid (null) value
    "   mov sp, 0x0",

    // setup page table
    "mov eax, [ap_pml4]",
    "mov edi, [eax]",
    "mov cr3, edi",

    // init FPU
    "   fninit",

    // load 32-bit GDT
    "   lgdt [gdt32_ptr]",

    // set CR4 flags
    "   mov eax, cr4",
    "   or eax, {cr4flags}",
    "   mov cr4, eax",

    // enable long mode in EFER
    "   mov ecx, {efer_num}",
    "   rdmsr",
    "   or eax, {efer_bits}",
    "   wrmsr",

    // set CR0 flags to enable paging and write protection
    "   mov ebx, cr0",
    "   or ebx, {cr0flags}",
    "   mov cr0, ebx",

    // 32-bit GDT
    ".align 16",
    "gdt32:",
    // TODO(eliza): would be nice to build the bits of the GDT entries in
    // Rust...
    "   .long 0, 0",
    "   .quad {gdt32_code}", // code segment
    "   .quad {gdt32_data}", // data segment
    "   .long 0x00000068, 0x00CF8900", // TSS
    "gdt32_ptr:",
    "   .word gdt32_ptr - gdt32 - 1", // size
    "   .word gdt32", // offset
    "ap_trampoline_end:",
    // ".code64", // reset to 64 bit code when exiting the asm block
    trampoline_addr = const AP_TRAMPOLINE_ADDR,
    cr4flags = const AP_CR4,
    cr0flags = const AP_CR0,
    efer_num = const Msr::ia32_efer().num,
    efer_bits = const EFER_LONG_MODE,
    gdt32_code = const segment::Descriptor::code_32()
        .with_ring(Ring::Ring0).bits(),
    gdt32_data = const segment::Descriptor::data_flat_16()
        .bits(),
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
