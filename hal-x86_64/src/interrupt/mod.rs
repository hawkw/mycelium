pub mod idt;
use crate::segment;
use core::{fmt, marker::PhantomData};
pub use idt::Idt;

#[derive(Debug)]
#[repr(C)]
pub struct Context<'a, T = ()> {
    registers: &'a mut Registers,
    code: T,
}

pub type ErrorCode = u64;

pub type PageFaultContext<'a> = Context<'a, PageFaultCode>;

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
    pub instruction_ptr: u64, // TODO(eliza): add VAddr
    _pad: [u16; 3],
    pub code_segment: segment::Selector,
    pub cpu_flags: u64, // TODO(eliza): rflags type?
    pub stack_ptr: u64, // TODO(eliza): add VAddr
    pub stack_segment: u64,
}

static mut IDT: idt::Idt = idt::Idt::new();

#[cold]
#[inline(never)]
extern "x86-interrupt" fn nop(cx: Context<'_>) {}

pub fn init(bootinfo: &impl hal_core::boot::BootInfo<Arch = crate::X64>) -> &'static mut idt::Idt {
    use core::fmt::Write;
    use hal_core::interrupt::Control;

    let mut writer = bootinfo.writer();

    writeln!(&mut writer, "\tintializing interrupts...").unwrap();

    unsafe {
        // (&mut IDT).descriptors[0x20].set_handler(nop as *const ());
        IDT.register_handler::<hal_core::interrupt::vectors::Test<Context<'_>>, Context<'_>>(
            |frame| {
                use crate::vga;

                let mut vga = vga::writer();
                vga.set_color(vga::ColorSpec::new(vga::Color::Blue, vga::Color::Black));
                writeln!(&mut vga, "lol im in ur test interrupt\n{:#?}", frame).unwrap();
                vga.set_color(vga::ColorSpec::new(vga::Color::Green, vga::Color::Black));
            },
        )
        .unwrap();

        writeln!(&mut writer, "{:?}", IDT.descriptors[69]).unwrap();
        IDT.load();
        IDT.enable();
    }

    writeln!(&mut writer, "\ttesting interrupts...").unwrap();
    unsafe {
        asm!("int $0" :: "i"(69) :: "volatile");
    }
    writeln!(&mut writer, "\tit worked?").unwrap();

    unsafe { &mut IDT }
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

impl<T> Interrupt<T> {
    /// Divide-by-zero interrupt (#D0)
    pub const DIVIDE_BY_ZERO: Interrupt = Interrupt::new_untyped(0);

    pub const DEBUG: Interrupt = Interrupt::new_untyped(1);

    /// Non-maskable interrupt.
    pub const NMI: Interrupt = Interrupt::new_untyped(2);

    pub const BREAKPOINT: Interrupt = Interrupt::new_untyped(3);

    pub const OVERFLOW: Interrupt = Interrupt::new_untyped(4);

    pub const BOUND_RANGE_EXCEEDED: Interrupt = Interrupt::new_untyped(5);

    pub const INVALID_OPCODE: Interrupt = Interrupt::new_untyped(6);

    /// A device not available exception
    pub const DEVICE_NOT_AVAILABLE: Interrupt = Interrupt::new_untyped(7);

    // TODO(eliza): can we enforce that this diverges?
    pub const DOUBLE_FAULT: Interrupt<ErrorCode> = Interrupt::new_untyped(8);

    /// On modern CPUs, this interrupt is reserved; this error fires a general
    /// protection fault instead.
    const COPROCESSOR_SEGMENT_OVERRUN: Interrupt = Interrupt::new_untyped(9);

    pub const INVALID_TSS: Interrupt<ErrorCode> = Interrupt::new_untyped(10);

    pub const SEGMENT_NOT_PRESENT: Interrupt<ErrorCode> = Interrupt::new_untyped(11);

    pub const STACK_SEGMENT_FAULT: Interrupt<ErrorCode> = Interrupt::new_untyped(12);

    pub const GENERAL_PROTECTION_FAULT: Interrupt<ErrorCode> = Interrupt::new_untyped(13);

    pub const PAGE_FAULT: Interrupt<PageFaultCode> = Interrupt::new_untyped(14);

    pub const X87_FPU_EXCEPTION_PENDING: Interrupt = Interrupt::new_untyped(16);

    pub const ALIGNMENT_CHECK: Interrupt<ErrorCode> = Interrupt::new_untyped(17);

    pub const MACHINE_CHECK: Interrupt<ErrorCode> = Interrupt::new_untyped(18);

    pub const SIMD_FLOATING_POINT: Interrupt = Interrupt::new_untyped(19);

    pub const VIRTUALIZATION_EXCEPTION: Interrupt = Interrupt::new_untyped(20);

    pub const SECURITY_EXCEPTION: Interrupt<ErrorCode> = Interrupt::new_untyped(30);

    pub(crate) const fn new_untyped(vector: u8) -> Self {
        Self {
            vector,
            _t: PhantomData,
        }
    }
}

// impl<T> hal_core::interrupt::Interrupt for Interrupt<T> {
//     type Ctrl = idt::Idt;
//     type Handler = Isr<T>;

//     fn vector(&self) -> u8 {
//         self.vector
//     }
// }

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
