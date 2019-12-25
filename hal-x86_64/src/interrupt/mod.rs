pub mod idt;
use crate::segment;
use core::fmt;

#[derive(Debug)]
#[repr(C)]
pub struct Context<'a, T = ()> {
    registers: &'a mut Registers,
    code: T,
}

pub type ErrorCode = u64;

/// An interrupt service routine.
pub type Isr<T> = extern "x86-interrupt" fn(&mut Context<T>);

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct PageFaultCode(u64);

#[repr(C)]
pub struct Registers {
    pub instruction_ptr: u64, // TODO(eliza): add VAddr
    _pad: [u16; 3],
    pub code_segment: segment::Selector,
    pub cpu_flags: u64, // TODO(eliza): rflags type?
    pub stack_ptr: u64, // TODO(eliza): add VAddr
    pub stack_segment: u64,
}

impl<'a, T> Context<'a, T> {
    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    /// # Safety
    ///
    /// Mutating the value of saved interrupt registers can cause
    /// undefined behavior.
    pub unsafe fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }
}

impl<'a> Context<'a, ErrorCode> {
    pub fn error_code(&self) -> ErrorCode {
        self.code
    }
}

impl<'a> Context<'a, PageFaultCode> {
    pub fn page_fault_code(&self) -> PageFaultCode {
        self.code
    }
}

impl fmt::Debug for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registers")
            .field(
                "instruction_ptr",
                &format_args!("{:#x}", self.instruction_ptr),
            )
            .field("code_segment", &self.code_segment)
            .field("cpu_flags", &format_args!("{:#b}", self.cpu_flags))
            .field("stack_ptr", &format_args!("{:#x}", self.stack_ptr))
            .field("stack_segment", &format_args!("{:#x}", self.stack_segment))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_is_correct_size() {
        use core::mem::size_of;
        assert_eq!(size_of::<Context<'_>>(), size_of::<&mut Registers>());
        assert_eq!(
            size_of::<Context<'_, ErrorCode>>(),
            size_of::<&mut Registers>() + size_of::<ErrorCode>()
        );
        assert_eq!(
            size_of::<Context<'_, PageFaultCode>>(),
            size_of::<&mut Registers>() + size_of::<PageFaultCode>()
        );
    }
}
