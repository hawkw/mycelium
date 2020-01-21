use crate::VAddr;
use core::fmt;

pub trait Context {
    // TODO(eliza): Registers trait
    type Registers: fmt::Debug + fmt::Display;

    fn registers(&self) -> &Self::Registers;
    unsafe fn registers_mut(&mut self) -> &mut Self::Registers;
}

pub trait PageFault: Context {
    fn fault_vaddr(&self) -> VAddr;

    // TODO(eliza): more
}

pub trait CodeFault: Context {
    fn is_user_mode(&self) -> bool;
    fn instruction_ptr(&self) -> VAddr;
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
