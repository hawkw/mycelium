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
#[inline(always)]
pub unsafe fn sti() {
    asm!("sti", options(nomem, nostack))
}

/// Perform one x86 `lidt` (*L*oad *I*interrupt *D*escriptor *T*able)
/// instruction.
///
/// `lidt` loads an interrupt descriptor table.
///
/// # Safety
///
/// - Intrinsics are inherently unsafe — this is just a less ugly way of writing
///   inline assembly.
/// - The provided `DtablePtr` must point to a valid IDT.
/// - The pointed IDT must not be deallocated or overwritten while it is active.
///
/// Prefer the higher-level [`interrupt::Idt::load`] API when
/// possible.
#[inline(always)]
pub(crate) unsafe fn lidt(ptr: super::DtablePtr) {
    asm!("lidt [{0}]", in(reg) &ptr, options(readonly, nostack, preserves_flags))
}

/// Perform one x86 `lidt` (*L*oad *G*lobal *D*escriptor *T*able)
/// instruction.
///
/// `lgdt` loads a GDT.
///
/// # Safety
///
/// - Intrinsics are inherently unsafe — this is just a less ugly way of writing
///   inline assembly.
/// - The provided `DtablePtr` must point to a valid GDT.
/// - The pointed IDT must not be deallocated or overwritten while it is active.
///
/// Prefer the higher-level [`segment::Gdt::load`] API when possible.
#[inline(always)]
pub(crate) unsafe fn lgdt(ptr: super::DtablePtr) {
    asm!("lgdt [{0}]", in(reg) &ptr, options(readonly, nostack, preserves_flags))
}

/// Perform one x86 `ltr` (*L*oad *T*ask *R*egister) instruction.
///
/// `ltr` loads a [task state segment (TSS)][tss] selector into the current task
/// register.
///
/// # Safety
///
/// - Intrinsics are inherently unsafe — this is just a less ugly way of writing
///   inline assembly.
/// - The provided [segment selector] must select a task state segment in the
///   [GDT].
/// - The pointed TSS must not be deallocated or overwritten while it is active.
///
/// Prefer the higher-level [`task::StateSegment::load`] API when possible.
///
/// [tss]: crate::task::StateSegment
/// [segment selector]: crate::segment::Selector
/// [GDT]: crate::segment::Gdt
#[inline(always)]
pub unsafe fn ltr(sel: crate::segment::Selector) {
    asm!("ltr {0:x}", in(reg) sel.bits(), options(nomem, nostack, preserves_flags))
}
