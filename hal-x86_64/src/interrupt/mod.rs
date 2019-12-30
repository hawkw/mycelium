pub mod idt;
pub mod pic;
use crate::{cpu, segment, vga, VAddr};
use core::{
    fmt::{self, Write},
    marker::PhantomData,
};
pub use idt::Idt;
pub use pic::CascadedPic;

use hal_core::interrupt::{ctx, Handlers};

#[derive(Debug)]
#[repr(C)]
pub struct Context<'a, T = ()> {
    registers: &'a mut Registers,
    code: T,
}

pub type ErrorCode = u64;

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
pub struct PageFaultCode(u64);

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
static mut TIMER: usize = 0;

struct TestHandlersImpl;

impl Handlers<crate::X64> for TestHandlersImpl {
    #[inline(never)]
    fn page_fault<C>(cx: C)
    where
        C: ctx::Context<Arch = crate::X64> + ctx::PageFault,
    {
        let mut vga = vga::writer();
        vga.set_color(vga::ColorSpec::new(vga::Color::Red, vga::Color::Black));
        log::error!("page fault\n{:#?}", cx.registers());
        vga.set_color(vga::ColorSpec::new(vga::Color::Green, vga::Color::Black));
    }

    #[inline(never)]
    fn code_fault<C>(cx: C)
    where
        C: ctx::Context<Arch = crate::X64> + ctx::CodeFault,
    {
        log::error!("code fault: {:?}\n{:#?}", cx.kind(), cx.registers());
        loop {}
    }

    #[inline(never)]
    fn double_fault<C>(cx: C) -> !
    where
        C: ctx::Context<Arch = crate::X64>,
    {
        panic!("double fault\n{:#?}", cx.registers());
    }

    #[inline(never)]
    fn timer_tick() {
        let timer = unsafe {
            TIMER += 1;
            TIMER
        };
        let seconds_hand = timer % 8;
        match seconds_hand {
            0 => {
                log::trace!("timer tick");
            }
            4 => {
                log::trace!("timer tock");
            }
            _ => {}
        }
    }

    #[inline(never)]
    fn keyboard_controller() {
        // load-bearing read - if we don't read from the keyboard controller it won't
        // send another interrupt on later keystrokes.
        //
        // 0x60 is a magic PC/AT number.
        let scancode = unsafe { cpu::Port::at(0x60).readb() };
        let mut vga = vga::writer();
        log::info!(
            // for now
            "got scancode {}. the time is now: {}",
            scancode,
            unsafe { TIMER }
        );
    }

    #[inline(never)]
    fn test_interrupt<C>(cx: C)
    where
        C: ctx::Context<Arch = crate::X64>,
    {
        let mut vga = vga::writer();
        log::info!("lol im in ur test interrupt\n{:4>#?}", cx.registers());
    }
}

pub fn init(bootinfo: &impl hal_core::boot::BootInfo<Arch = crate::X64>) -> &'static mut idt::Idt {
    use hal_core::interrupt::Control;
    log::info!("configuring 8259 PIC interrupts...");

    unsafe {
        PIC.set_irq_addresses(0x20, 0x28);
        // functionally a no-op, since interrupts from PC/AT PIC are enabled at boot, just being
        // clear for you, the reader, that at this point they are definitely intentionally enabled.
        PIC.enable();
    }

    log::info!("intializing IDT...");

    unsafe {
        IDT.register_handlers::<TestHandlersImpl>().unwrap();
        log::debug!("{:#?}", IDT.descriptors[69]);
        IDT.load();
        IDT.enable();
    }

    log::debug!("testing interrupts...");
    unsafe {
        asm!("int $0" :: "i"(69) :: "volatile");
    }
    // loop {}
    log::debug!("it worked");

    unsafe { &mut IDT }
}

impl<'a, T> hal_core::interrupt::Context for Context<'a, T> {
    type Arch = crate::X64;
    type Registers = Registers;

    fn registers(&self) -> &Registers {
        &self.registers
    }

    /// # Safety
    ///
    /// Mutating the value of saved interrupt registers can cause
    /// undefined behavior.
    unsafe fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    fn instruction_ptr(&self) -> VAddr {
        self.registers.instruction_ptr
    }
}

impl<'a> hal_core::interrupt::ctx::PageFault for Context<'a, PageFaultCode> {
    fn fault_vaddr(&self) -> crate::VAddr {
        unimplemented!("eliza")
    }
}

impl<'a> hal_core::interrupt::ctx::CodeFault for Context<'a, ctx::CodeFaultKind> {
    fn is_user_mode(&self) -> bool {
        false // TODO(eliza)
    }

    fn kind(&self) -> ctx::CodeFaultKind {
        self.code
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
    type Arch = crate::X64;
    // type Vector = u8;

    unsafe fn disable(&mut self) {
        asm!("cli" :::: "volatile");
    }

    unsafe fn enable(&mut self) {
        asm!("sti" :::: "volatile");
    }

    fn is_enabled(&self) -> bool {
        unimplemented!("eliza do this one!!!")
    }

    fn register_handlers<H>(&mut self) -> Result<(), hal_core::interrupt::RegistrationError>
    where
        H: Handlers<Self::Arch>,
    {
        extern "x86-interrupt" fn page_fault_isr<H: Handlers<crate::X64>>(
            registers: &mut Registers,
            code: PageFaultCode,
        ) {
            H::page_fault(Context { registers, code });
        }

        extern "x86-interrupt" fn double_fault_isr<H: Handlers<crate::X64>>(
            registers: &mut Registers,
            code: u64,
        ) {
            H::double_fault(Context { registers, code });
        }

        extern "x86-interrupt" fn divide_error_isr<H: Handlers<crate::X64>>(
            registers: &mut Registers,
        ) {
            H::code_fault(Context {
                registers,
                code: ctx::CodeFaultKind::Division,
            });
        }

        extern "x86-interrupt" fn overflow_isr<H: Handlers<crate::X64>>(registers: &mut Registers) {
            H::code_fault(Context {
                registers,
                code: ctx::CodeFaultKind::Overflow,
            });
        }

        extern "x86-interrupt" fn invalid_opcode_isr<H: Handlers<crate::X64>>(
            registers: &mut Registers,
        ) {
            H::code_fault(Context {
                registers,
                code: ctx::CodeFaultKind::InvalidInstruction,
            });
        }

        extern "x86-interrupt" fn timer_isr<H: Handlers<crate::X64>>(_regs: &mut Registers) {
            H::timer_tick();
            unsafe {
                PIC.end_interrupt(0x20);
            }
        }

        extern "x86-interrupt" fn keyboard_isr<H: Handlers<crate::X64>>(_regs: &mut Registers) {
            H::keyboard_controller();
            unsafe {
                PIC.end_interrupt(0x21);
            }
        }

        extern "x86-interrupt" fn test_isr<H: Handlers<crate::X64>>(registers: &mut Registers) {
            H::test_interrupt(Context {
                registers,
                code: (),
            });
        }

        // code fault ISRs
        self.descriptors[Self::DIVIDE_BY_ZERO].set_handler(divide_error_isr::<H> as *const ());
        self.descriptors[Self::OVERFLOW].set_handler(overflow_isr::<H> as *const ());
        self.descriptors[Self::INVALID_OPCODE].set_handler(invalid_opcode_isr::<H> as *const ());

        self.descriptors[0x20].set_handler(timer_isr::<H> as *const ());
        self.descriptors[0x21].set_handler(keyboard_isr::<H> as *const ());
        self.descriptors[69].set_handler(test_isr::<H> as *const ());
        self.descriptors[Self::PAGE_FAULT].set_handler(page_fault_isr::<H> as *const ());
        self.descriptors[Self::DOUBLE_FAULT].set_handler(double_fault_isr::<H> as *const ());
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

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

    #[test]
    fn registers_is_correct_size() {
        assert_eq!(size_of::<Registers>(), 40);
    }
}
