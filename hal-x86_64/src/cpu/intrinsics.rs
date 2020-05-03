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
    llvm_asm!("hlt" :::: "volatile")
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
    llvm_asm!("cli" :::: "volatile")
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
    llvm_asm!("sti" :::: "volatile")
}
