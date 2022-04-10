use crate::VAddr;
use core::fmt;

pub trait Context {
    // TODO(eliza): Registers trait
    type Registers: fmt::Debug + fmt::Display;

    fn registers(&self) -> &Self::Registers;

    /// # Safety
    ///
    /// Mutating the value of saved interrupt registers can cause
    /// undefined behavior.
    unsafe fn registers_mut(&mut self) -> &mut Self::Registers;
}

pub trait PageFault: Context {
    fn fault_vaddr(&self) -> VAddr;
    fn debug_error_code(&self) -> &dyn fmt::Debug;
    // TODO(eliza): more
}

/// Trait representing a fault caused by the currently executing code.
pub trait CodeFault: Context {
    /// Returns `true` if the code fault occurred while executing in user
    /// mode code.
    fn is_user_mode(&self) -> bool;

    /// Returns the virtual address of the instruction pointer where the
    /// fault occurred.
    fn instruction_ptr(&self) -> VAddr;

    /// Returns a static string describing the kind of code fault.
    fn fault_kind(&self) -> &'static str;

    /// Returns a dynamically formatted message if additional information about
    /// the fault is available. Otherwise, this returns `None`.
    ///
    /// # Default Implementation
    ///
    /// Returns `None`.
    fn details(&self) -> Option<&dyn fmt::Display> {
        None
    }
}

#[non_exhaustive]
pub enum CodeFaultKind {
    /// The code fault was a division by zero.
    Division,
    /// The code fault was caused by an invalid instruction.
    InvalidInstruction,
    Overflow,
    Alignment,
    Other(&'static str),
}
