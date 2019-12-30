use crate::Architecture;
use core::fmt;

pub trait Context {
    type Arch: Architecture;
    // TODO(eliza): Registers trait
    type Registers: fmt::Debug;

    fn registers(&self) -> &Self::Registers;
    unsafe fn registers_mut(&mut self) -> &mut Self::Registers;
}

pub trait PageFault: Context {
    fn fault_vaddr(&self) -> <<Self as Context>::Arch as Architecture>::VAddr;

    // TODO(eliza): more
}

pub trait CodeFault: Context {
    fn is_user_mode(&self) -> bool;
    fn instruction_ptr(&self) -> <<Self as Context>::Arch as Architecture>::VAddr;
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
