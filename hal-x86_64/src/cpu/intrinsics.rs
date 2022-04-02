use core::arch::asm;

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
    asm!("cli")
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
    asm!("sti")
}
