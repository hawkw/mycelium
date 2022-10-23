use crate::{cpu, mm, segment, PAddr, VAddr};
use core::{arch::asm, marker::PhantomData, time::Duration};
use hal_core::interrupt::Control;
use hal_core::interrupt::{ctx, Handlers};
use mycelium_util::{
    bits, fmt,
    sync::{spin, InitOnce},
};

pub mod apic;
pub mod idt;
pub mod pic;

use self::apic::{IoApic, LocalApic};
pub use idt::Idt;
pub use pic::CascadedPic;

#[derive(Debug)]
pub struct Controller {
    model: InterruptModel,
}

#[derive(Debug)]
#[repr(C)]
pub struct Context<'a, T = ()> {
    registers: &'a mut Registers,
    code: T,
}

pub type ErrorCode = u64;

pub struct CodeFault<'a> {
    kind: &'static str,
    error_code: Option<&'a dyn fmt::Display>,
}

/// An interrupt service routine.
pub type Isr<T> = extern "x86-interrupt" fn(&mut Context<T>);

#[derive(Debug)]
#[repr(C)]
pub struct Interrupt<T = ()> {
    vector: u8,
    _t: PhantomData<T>,
}

/// The interrupt controller's active interrupt model.
#[derive(Debug)]

enum InterruptModel {
    /// Interrupts are handled by the [8259 Programmable Interrupt Controller
    /// (PIC)](pic).
    Pic(spin::Mutex<pic::CascadedPic>),
    /// Interrupts are handled by the [local] and [I/O] [Advanced Programmable
    /// Interrupt Controller (APIC)s][apics].
    ///
    /// [local]: apic::LocalApic
    /// [I/O]: apic::IoApic
    /// [apics]: apic
    Apic {
        local: apic::LocalApic,
        // TODO(eliza): allow further configuration of the I/O APIC (e.g.
        // masking/unmasking stuff...)
        #[allow(dead_code)]
        io: spin::Mutex<apic::IoApic>,
    },
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct PageFaultCode(u32);

bits::bitfield! {
    /// Error code set by the "Invalid TSS", "Segment Not Present", "Stack-Segment
    /// Fault", and "General Protection Fault" faults.
    ///
    /// This includes a segment selector index, and includes 2 bits describing
    /// which table the segment selector references.
    pub struct SelectorErrorCode<u16> {
        const EXTERNAL: bool;
        const TABLE: cpu::DescriptorTable;
        const INDEX = 13;
    }
}

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

static IDT: spin::Mutex<idt::Idt> = spin::Mutex::new(idt::Idt::new());
static INTERRUPT_CONTROLLER: InitOnce<Controller> = InitOnce::uninitialized();

impl Controller {
    // const DEFAULT_IOAPIC_BASE_PADDR: u64 = 0xFEC00000;

    #[tracing::instrument(level = "info", name = "interrupt::Controller::init")]
    pub fn init<H: Handlers<Registers>>() {
        tracing::info!("intializing IDT...");

        let mut idt = IDT.lock();
        idt.register_handlers::<H>().unwrap();
        unsafe {
            idt.load_raw();
        }
    }

    pub fn enable_hardware_interrupts(acpi: Option<&acpi::InterruptModel>) -> &'static Self {
        let mut pics = pic::CascadedPic::new();
        // regardless of whether APIC or PIC interrupt handling will be used,
        // the PIC interrupt vectors must be remapped so that they do not
        // conflict with CPU exceptions.
        unsafe {
            tracing::debug!(
                big = Idt::PIC_BIG_START,
                little = Idt::PIC_LITTLE_START,
                "remapping PIC interrupt vectors"
            );
            pics.set_irq_address(Idt::PIC_BIG_START as u8, Idt::PIC_LITTLE_START as u8);
        }

        let model = match acpi {
            Some(acpi::InterruptModel::Apic(apic_info)) => {
                tracing::info!("detected APIC interrupt model");

                // disable the 8259 PICs so that we can use APIC interrupts instead
                unsafe {
                    pics.disable();
                }
                tracing::info!("disabled 8259 PICs");

                // configure the I/O APIC
                let mut io = {
                    // TODO(eliza): consider actually using other I/O APICs? do
                    // we need them for anything??
                    let io_apic = &apic_info.io_apics[0];
                    let paddr = PAddr::from_u64(io_apic.address as u64);
                    let vaddr = mm::kernel_vaddr_of(paddr);
                    IoApic::new(vaddr)
                };

                io.map_isa_irqs(Idt::IOAPIC_START as u8);
                // unmask the PIT timer vector --- we'll need this for calibrating
                // the local APIC timer...
                io.set_masked(IoApic::PIT_TIMER_IRQ, false);

                // enable the local APIC
                let local = LocalApic::new();
                local.enable(Idt::LOCAL_APIC_SPURIOUS as u8);

                InterruptModel::Apic {
                    local,
                    io: spin::Mutex::new(io),
                }
            }
            model => {
                if model.is_none() {
                    tracing::warn!("platform does not support ACPI; falling back to 8259 PIC");
                } else {
                    tracing::warn!(
                        "ACPI does not indicate APIC interrupt model; falling back to 8259 PIC"
                    )
                }
                tracing::info!("configuring 8259 PIC interrupts...");

                unsafe {
                    // functionally a no-op, since interrupts from PC/AT PIC are enabled at boot, just being
                    // clear for you, the reader, that at this point they are definitely intentionally enabled.
                    pics.enable();
                }
                InterruptModel::Pic(spin::Mutex::new(pics))
            }
        };
        tracing::trace!(interrupt_model = ?model);

        unsafe {
            crate::cpu::intrinsics::sti();
        }

        let controller = Self { model };
        INTERRUPT_CONTROLLER.init(controller)
    }

    /// Starts a periodic timer which fires the `timer_tick` interrupt of the
    /// provided [`Handlers`] every time `interval` elapses.
    pub fn start_periodic_timer(
        &self,
        interval: Duration,
    ) -> Result<(), crate::time::InvalidDuration> {
        match self.model {
            InterruptModel::Pic(_) => crate::time::PIT.lock().start_periodic_timer(interval),
            InterruptModel::Apic { ref local, .. } => {
                local.start_periodic_timer(interval, Idt::LOCAL_APIC_TIMER as u8)
            }
        }
    }
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

impl<'a> ctx::CodeFault for Context<'a, CodeFault<'a>> {
    fn is_user_mode(&self) -> bool {
        false // TODO(eliza)
    }

    fn instruction_ptr(&self) -> crate::VAddr {
        self.registers.instruction_ptr
    }

    fn fault_kind(&self) -> &'static str {
        self.code.kind
    }

    fn details(&self) -> Option<&dyn fmt::Display> {
        self.code.error_code
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
        tracing::trace!("interrupts enabled");
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
                        error_code: Some(&code),
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

        extern "x86-interrupt" fn pit_timer_isr<H: Handlers<Registers>>(_regs: Registers) {
            use core::sync::atomic::Ordering;
            // if we weren't trying to do a PIT sleep, handle the timer tick
            // instead.
            let was_sleeping = crate::time::pit::SLEEPING
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
            if !was_sleeping {
                H::timer_tick();
            } else {
                tracing::trace!("PIT sleep completed");
            }
            unsafe {
                match INTERRUPT_CONTROLLER.get_unchecked().model {
                    InterruptModel::Pic(ref pics) => {
                        pics.lock().end_interrupt(Idt::PIC_PIT_TIMER as u8)
                    }
                    InterruptModel::Apic { ref local, .. } => local.end_interrupt(),
                }
            }
        }

        extern "x86-interrupt" fn apic_timer_isr<H: Handlers<Registers>>(_regs: Registers) {
            H::timer_tick();
            unsafe {
                match INTERRUPT_CONTROLLER.get_unchecked().model {
                    InterruptModel::Pic(_) => unreachable!(),
                    InterruptModel::Apic { ref local, .. } => local.end_interrupt(),
                }
            }
        }

        extern "x86-interrupt" fn keyboard_isr<H: Handlers<Registers>>(_regs: Registers) {
            // 0x60 is a magic PC/AT number.
            static PORT: cpu::Port = cpu::Port::at(0x60);
            // load-bearing read - if we don't read from the keyboard controller it won't
            // send another interrupt on later keystrokes.
            let scancode = unsafe { PORT.readb() };
            H::ps2_keyboard(scancode);
            unsafe {
                match INTERRUPT_CONTROLLER.get_unchecked().model {
                    InterruptModel::Pic(ref pics) => pics.lock().end_interrupt(0x21),
                    InterruptModel::Apic { ref local, .. } => local.end_interrupt(),
                }
            }
        }

        extern "x86-interrupt" fn test_isr<H: Handlers<Registers>>(mut registers: Registers) {
            H::test_interrupt(Context {
                registers: &mut registers,
                code: (),
            });
        }

        extern "x86-interrupt" fn invalid_tss_isr<H: Handlers<Registers>>(
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
            let selector = SelectorErrorCode(code as u16);
            tracing::error!(?selector, "invalid task-state segment!");

            let msg = selector.named("task-state segment (TSS)");
            let code = CodeFault {
                error_code: Some(&msg),
                kind: "Invalid TSS (0xA)",
            };
            H::code_fault(Context {
                registers: &mut registers,
                code,
            });
        }

        extern "x86-interrupt" fn segment_not_present_isr<H: Handlers<Registers>>(
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
            let selector = SelectorErrorCode(code as u16);
            tracing::error!(?selector, "a segment was not present!");

            let msg = selector.named("stack segment");
            let code = CodeFault {
                error_code: Some(&msg),
                kind: "Segment Not Present (0xB)",
            };
            H::code_fault(Context {
                registers: &mut registers,
                code,
            });
        }

        extern "x86-interrupt" fn stack_segment_isr<H: Handlers<Registers>>(
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
            let selector = SelectorErrorCode(code as u16);
            tracing::error!(?selector, "a stack-segment fault is happening");

            let msg = selector.named("stack segment");
            let code = CodeFault {
                error_code: Some(&msg),
                kind: "Stack-Segment Fault (0xC)",
            };
            H::code_fault(Context {
                registers: &mut registers,
                code,
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

            let segment = if code > 0 {
                Some(SelectorErrorCode(code as u16))
            } else {
                None
            };

            tracing::error!(?segment, "lmao, a general protection fault is happening");
            let error_code = segment.map(|seg| seg.named("selector"));
            let code = CodeFault {
                error_code: error_code.as_ref().map(|code| code as &dyn fmt::Display),
                kind: "General Protection Fault (0xD)",
            };
            H::code_fault(Context {
                registers: &mut registers,
                code,
            });
        }

        extern "x86-interrupt" fn spurious_isr() {
            tracing::trace!("spurious");
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

        self.set_isr(Self::PIC_PIT_TIMER, pit_timer_isr::<H> as *const ());
        self.set_isr(Self::IOAPIC_PIT_TIMER, pit_timer_isr::<H> as *const ());
        self.set_isr(
            Self::LOCAL_APIC_SPURIOUS as usize,
            spurious_isr as *const (),
        );
        self.set_isr(
            Self::LOCAL_APIC_TIMER as usize,
            apic_timer_isr::<H> as *const (),
        );
        self.set_isr(0x21, keyboard_isr::<H> as *const ());
        self.set_isr(69, test_isr::<H> as *const ());
        self.set_isr(Self::PAGE_FAULT, page_fault_isr::<H> as *const ());
        self.set_isr(Self::INVALID_TSS, invalid_tss_isr::<H> as *const ());
        self.set_isr(
            Self::SEGMENT_NOT_PRESENT,
            segment_not_present_isr::<H> as *const (),
        );
        self.set_isr(
            Self::STACK_SEGMENT_FAULT,
            stack_segment_isr::<H> as *const (),
        );
        self.set_isr(Self::GENERAL_PROTECTION_FAULT, gpf_isr::<H> as *const ());
        self.set_isr(Self::DOUBLE_FAULT, double_fault_isr::<H> as *const ());
        Ok(())
    }
}

impl fmt::Debug for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            instruction_ptr,
            code_segment,
            stack_ptr,
            stack_segment,
            _pad: _,
            cpu_flags,
            _pad2: _,
        } = self;
        f.debug_struct("Registers")
            .field("instruction_ptr", instruction_ptr)
            .field("code_segment", code_segment)
            .field("cpu_flags", &format_args!("{cpu_flags:#b}"))
            .field("stack_ptr", stack_ptr)
            .field("stack_segment", stack_segment)
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

// === impl SelectorErrorCode ===

impl SelectorErrorCode {
    #[inline]
    fn named(self, segment_kind: &'static str) -> NamedSelectorErrorCode {
        NamedSelectorErrorCode {
            segment_kind,
            code: self,
        }
    }

    fn display(&self) -> impl fmt::Display {
        struct PrettyErrorCode(SelectorErrorCode);

        impl fmt::Display for PrettyErrorCode {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let table = self.0.get(SelectorErrorCode::TABLE);
                let index = self.0.get(SelectorErrorCode::INDEX);
                write!(f, "{table} index {index}")?;
                if self.0.get(SelectorErrorCode::EXTERNAL) {
                    f.write_str(" (from an external source)")?;
                }
                write!(f, " (error code {:#b})", self.0.bits())?;

                Ok(())
            }
        }

        PrettyErrorCode(*self)
    }
}

struct NamedSelectorErrorCode {
    segment_kind: &'static str,
    code: SelectorErrorCode,
}

impl fmt::Display for NamedSelectorErrorCode {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.segment_kind, self.code.display())
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
