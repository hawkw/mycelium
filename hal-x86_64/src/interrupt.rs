pub mod idt;
pub mod pic;
use crate::{segment, VAddr};
use core::{arch::asm, fmt, marker::PhantomData};
pub use idt::Idt;
pub use pic::CascadedPic;

use hal_core::interrupt::{ctx, Handlers};

pub type Control = &'static mut Idt;

#[derive(Debug)]
#[repr(C)]
pub struct Context<'a, T = ()> {
    registers: &'a mut Registers,
    code: T,
}

pub type ErrorCode = u64;

#[derive(Debug)]
pub struct CodeFault {
    kind: &'static str,
    #[allow(dead_code)] // TODO(eliza): actually use this lol
    error_code: Option<u64>,
}

/// An interrupt service routine.
pub type Isr<T> = extern "x86-interrupt" fn(&mut Context<T>);

#[derive(Debug)]
#[repr(C)]
pub struct Interrupt<T = ()> {
    vector: u8,
    _t: PhantomData<T>,
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct PageFaultCode(u32);

#[repr(C)]
pub struct Registers {
    pub instruction_ptr: VAddr, // TODO(eliza): add VAddr
    pub code_segment: segment::Selector,
    _pad: [u16; 3],
    pub cpu_flags: u64,   // TODO(eliza): rflags type?
    pub stack_ptr: VAddr, // TODO(eliza): add VAddr
    pub stack_segment: segment::Selector,
    _pad2: [u16; 3],
}

static mut IDT: idt::Idt = idt::Idt::new();
static mut PIC: pic::CascadedPic = pic::CascadedPic::new();

pub fn init<H: Handlers<Registers>>() -> Control {
    use hal_core::interrupt::Control;

    let span = tracing::info_span!("interrupts::init");
    let _enter = span.enter();

    tracing::info!("configuring 8259 PIC interrupts...");

    unsafe {
        PIC.set_irq_address(0x20, 0x28);
        // functionally a no-op, since interrupts from PC/AT PIC are enabled at boot, just being
        // clear for you, the reader, that at this point they are definitely intentionally enabled.
        PIC.enable();
    }

    tracing::info!("intializing IDT...");

    unsafe {
        IDT.register_handlers::<H>().unwrap();
        IDT.load();
        IDT.enable();
    }

    unsafe { &mut IDT }
}

impl<'a, T> hal_core::interrupt::Context for Context<'a, T> {
    type Registers = Registers;

    fn registers(&self) -> &Registers {
        self.registers
    }

    /// # Safety
    ///
    /// Mutating the value of saved interrupt registers can cause
    /// undefined behavior.
    unsafe fn registers_mut(&mut self) -> &mut Registers {
        self.registers
    }
}

impl<'a> ctx::PageFault for Context<'a, PageFaultCode> {
    fn fault_vaddr(&self) -> crate::VAddr {
        unimplemented!("eliza")
    }

    fn debug_error_code(&self) -> &dyn fmt::Debug {
        &self.code
    }
}

impl<'a> ctx::CodeFault for Context<'a, CodeFault> {
    fn is_user_mode(&self) -> bool {
        false // TODO(eliza)
    }

    fn instruction_ptr(&self) -> crate::VAddr {
        self.registers.instruction_ptr
    }

    fn fault_kind(&self) -> &'static str {
        self.code.kind
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

impl hal_core::interrupt::Control for Idt {
    // type Vector = u8;
    type Registers = Registers;

    #[inline]
    unsafe fn disable(&mut self) {
        crate::cpu::intrinsics::cli();
    }

    #[inline]
    unsafe fn enable(&mut self) {
        crate::cpu::intrinsics::sti();
    }

    fn is_enabled(&self) -> bool {
        unimplemented!("eliza do this one!!!")
    }

    fn register_handlers<H>(&mut self) -> Result<(), hal_core::interrupt::RegistrationError>
    where
        H: Handlers<Registers>,
    {
        macro_rules! gen_code_faults {
            ($self:ident, $h:ty, $($vector:path => fn $name:ident($($rest:tt)+),)+) => {
                $(
                    gen_code_faults! {@ $name($($rest)+); }
                    $self.set_isr($vector, $name::<$h> as *const ());
                )+
            };
            (@ $name:ident($kind:literal);) => {
                extern "x86-interrupt" fn $name<H: Handlers<Registers>>(mut registers: Registers) {
                    let code = CodeFault {
                        error_code: None,
                        kind: $kind,
                    };
                    H::code_fault(Context { registers: &mut registers, code });
                }
            };
            (@ $name:ident($kind:literal, code);) => {
                extern "x86-interrupt" fn $name<H: Handlers<Registers>>(
                    mut registers: Registers,
                    code: u64,
                ) {
                    let code = CodeFault {
                        error_code: Some(code),
                        kind: $kind,
                    };
                    H::code_fault(Context {  registers: &mut registers, code });
                }
            };
        }

        let span = tracing::debug_span!("Idt::register_handlers");
        let _enter = span.enter();

        extern "x86-interrupt" fn page_fault_isr<H: Handlers<Registers>>(
            mut registers: Registers,
            code: PageFaultCode,
        ) {
            H::page_fault(Context {
                registers: &mut registers,
                code,
            });
        }

        extern "x86-interrupt" fn double_fault_isr<H: Handlers<Registers>>(
            mut registers: Registers,
            code: u64,
        ) {
            H::double_fault(Context {
                registers: &mut registers,
                code,
            });
        }

        extern "x86-interrupt" fn timer_isr<H: Handlers<Registers>>(_regs: Registers) {
            H::timer_tick();
            unsafe {
                PIC.end_interrupt(0x20);
            }
        }

        extern "x86-interrupt" fn keyboard_isr<H: Handlers<Registers>>(_regs: Registers) {
            H::keyboard_controller();
            unsafe {
                PIC.end_interrupt(0x21);
            }
        }

        extern "x86-interrupt" fn test_isr<H: Handlers<Registers>>(mut registers: Registers) {
            H::test_interrupt(Context {
                registers: &mut registers,
                code: (),
            });
        }

        extern "x86-interrupt" fn gpf_isr<H: Handlers<Registers>>(
            mut registers: Registers,
            code: u64,
        ) {
            unsafe {
                // Safety: who cares!

                crate::vga::writer().force_unlock();
                if let Some(com1) = crate::serial::com1() {
                    com1.force_unlock();
                }
            }
            tracing::error!(code = ?&format_args!("{:x}", code), "lmao, a general protection fault is happening");
            let code = CodeFault {
                error_code: Some(code),
                kind: "General Protection Fault (0xD)",
            };
            H::code_fault(Context {
                registers: &mut registers,
                code,
            });
        }

        gen_code_faults! {
            self, H,
            Self::DIVIDE_BY_ZERO => fn div_0_isr("Divide-By-Zero (0x0)"),
            Self::OVERFLOW => fn overflow_isr("Overflow (0x4)"),
            Self::BOUND_RANGE_EXCEEDED => fn br_isr("Bound Range Exceeded (0x5)"),
            Self::INVALID_OPCODE => fn ud_isr("Invalid Opcode (0x6)"),
            Self::DEVICE_NOT_AVAILABLE => fn no_fpu_isr("Device (FPU) Not Available (0x7)"),
            Self::ALIGNMENT_CHECK => fn alignment_check_isr("Alignment Check (0x11)", code),
            Self::SIMD_FLOATING_POINT => fn simd_fp_exn_isr("SIMD Floating-Point Exception (0x13)"),
            Self::X87_FPU_EXCEPTION => fn x87_exn_isr("x87 Floating-Point Exception (0x10)"),
        }

        self.set_isr(0x20, timer_isr::<H> as *const ());
        self.set_isr(0x21, keyboard_isr::<H> as *const ());
        self.set_isr(69, test_isr::<H> as *const ());
        self.set_isr(Self::PAGE_FAULT, page_fault_isr::<H> as *const ());
        self.set_isr(Self::GENERAL_PROTECTION_FAULT, gpf_isr::<H> as *const ());
        self.set_isr(Self::DOUBLE_FAULT, double_fault_isr::<H> as *const ());
        Ok(())
    }
}

impl fmt::Debug for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registers")
            .field("instruction_ptr", &self.instruction_ptr)
            .field("code_segment", &self.code_segment)
            .field("cpu_flags", &format_args!("{:#b}", self.cpu_flags))
            .field("stack_ptr", &self.stack_ptr)
            .field("stack_segment", &self.stack_segment)
            .finish()
    }
}

impl fmt::Display for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  rip:   {:?}", self.instruction_ptr)?;
        writeln!(f, "  cs:    {:?}", self.code_segment)?;
        writeln!(f, "  flags: {:#b}", self.cpu_flags)?;
        writeln!(f, "  rsp:   {:?}", self.stack_ptr)?;
        writeln!(f, "  ss:    {:?}", self.stack_segment)?;
        Ok(())
    }
}

pub fn fire_test_interrupt() {
    unsafe { asm!("int {0}", const 69) }
}

impl fmt::Debug for PageFaultCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageFaultCode({:#b})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn registers_is_correct_size() {
        assert_eq!(size_of::<Registers>(), 40);
    }
}
