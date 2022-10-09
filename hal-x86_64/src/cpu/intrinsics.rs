use super::DtablePtr;
use core::arch::asm;
pub use core::arch::x86_64::{
    _rdrand16_step as rdrand16_step, _rdrand32_step as rdrand32_step,
    _rdrand64_step as rdrand64_step, _rdtsc as rdtsc,
};

/// Perform one x86 `hlt` instruction.
///
/// # Safety
///
/// Intrinsics are inherently unsafe — this is just a less ugly way of writing
/// inline assembly.
///
/// Also...this halts the CPU.
#[inline(always)]
pub unsafe fn hlt() {
    asm!("hlt")
}

/// Perform one x86 `cli` instruction.
///
/// `cli` disables CPU interrupts.
///
/// # Safety
///
/// Intrinsics are inherently unsafe — this is just a less ugly way of writing
/// inline assembly. Also, this does not guarantee that interrupts will be
/// re-enabled, ever.
///
/// Prefer the higher-level [`interrupt::Control::enter_critical`] API when
/// possible.
///
/// [`interrupt::Control::enter_critical`]: crate::interrupt::Idt#method.enter_critical
#[inline(always)]
pub unsafe fn cli() {
    asm!("cli", options(nomem, nostack))
}

/// Perform one x86 `sti` instruction.
///
/// `sti` enables CPU interrupts.
///
/// # Safety
///
/// Intrinsics are inherently unsafe — this is just a less ugly way of writing
/// inline assembly.
///
/// Prefer the higher-level [`interrupt::Control::enter_critical`] API when
/// possible.
///
/// [`interrupt::Control::enter_critical`]: crate::interrupt::Idt#method.enter_critical
#[inline(always)]
pub unsafe fn sti() {
    asm!("sti", options(nomem, nostack))
}

/// Perform one x86 `lidt` (*L*oad *I*interrupt *D*escriptor *T*able)
/// instruction.
///
/// `lidt` loads an [interrupt descriptor table (IDT)][IDT] from a [`DtablePtr`].
///
/// # Safety
///
/// - Intrinsics are inherently unsafe — this is just a less ugly way of writing
///   inline assembly.
/// - The provided [`DtablePtr`] must point to a valid [IDT].
/// - The pointed [IDT] must not be deallocated or overwritten while it is active.
///
/// Prefer the higher-level [`interrupt::Idt::load`] API when
/// possible.
///
/// [IDT]: crate::interrupt::Idt
/// [`interrupt::Idt::load`]: crate::interrupt::Idt::load
#[inline(always)]
pub(crate) unsafe fn lidt(ptr: DtablePtr) {
    asm!("lidt [{0}]", in(reg) &ptr, options(readonly, nostack, preserves_flags))
}

/// Perform one x86 `lidt` (*L*oad *G*lobal *D*escriptor *T*able)
/// instruction.
///
/// `lgdt` loads a [global descriptor table (GDT)][GDT] from a [`DtablePtr`].
///
/// # Safety
///
/// - Intrinsics are inherently unsafe — this is just a less ugly way of writing
///   inline assembly.
/// - The provided [`DtablePtr`] must point to a valid [GDT].
/// - The pointed [GDT] must not be deallocated or overwritten while it is active.
///
/// Prefer the higher-level [`segment::Gdt::load`] API when possible.
///
/// [GDT]: crate::segment::Gdt
/// [`segment::Gdt::load`]: crate::segment::Gdt::load
#[inline(always)]
pub(crate) unsafe fn lgdt(ptr: DtablePtr) {
    asm!("lgdt [{0}]", in(reg) &ptr, options(readonly, nostack, preserves_flags))
}

/// Perform one x86 `ltr` (*L*oad *T*ask *R*egister) instruction.
///
/// `ltr` loads a [task state segment (TSS)][TSS] selector into the current task
/// register.
///
/// # Safety
///
/// - Intrinsics are inherently unsafe — this is just a less ugly way of writing
///   inline assembly.
/// - The provided [segment selector] must select a task state segment in the
///   [GDT].
/// - The pointed [TSS] must not be deallocated or overwritten while it is active.
///
/// Prefer the higher-level [`task::StateSegment::load_tss`] API when possible.
///
/// [TSS]: crate::task::StateSegment
/// [segment selector]: crate::segment::Selector
/// [GDT]: crate::segment::Gdt
/// [`task::StateSegment::load_tss`]: crate::task::StateSegment::load_tss
#[inline(always)]
pub unsafe fn ltr(sel: crate::segment::Selector) {
    asm!("ltr {0:x}", in(reg) sel.bits(), options(nomem, nostack, preserves_flags))
}
