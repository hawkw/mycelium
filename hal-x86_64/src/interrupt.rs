use crate::{cpu, mm, segment, time, VAddr};
use core::{arch::asm, marker::PhantomData, time::Duration};
use hal_core::interrupt::Control;
use hal_core::interrupt::{ctx, Handlers};
use hal_core::mem::page;
use mycelium_util::{
    bits, fmt,
    sync::{
        blocking::{Mutex, MutexGuard},
        spin::Spinlock,
        InitOnce,
    },
};

pub mod apic;
pub mod idt;
pub mod pic;

use self::apic::{IoApicSet, LocalApic};
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

#[derive(Debug, thiserror::Error)]
pub enum PeriodicTimerError {
    #[error("could not start PIT periodic timer: {0}")]
    Pit(#[from] time::PitError),
    #[error(transparent)]
    InvalidDuration(#[from] time::InvalidDuration),
    #[error("could access local APIC: {0}")]
    Apic(#[from] apic::local::LocalApicError),
    #[error("could not start local APIC periodic timer: {0}")]
    ApicTimer(#[from] apic::local::TimerError),
}

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
    Pic(Mutex<pic::CascadedPic, Spinlock>),
    /// Interrupts are handled by the [local] and [I/O] [Advanced Programmable
    /// Interrupt Controller (APIC)s][apics].
    ///
    /// [local]: apic::LocalApic
    /// [I/O]: apic::IoApic
    /// [apics]: apic
    Apic {
        io: apic::IoApicSet,
        local: apic::local::Handle,
    },
}

bits::bitfield! {
    pub struct PageFaultCode<u32> {
        /// When set, the page fault was caused by a page-protection violation.
        /// When not set, it was caused by a non-present page.
        pub const PRESENT: bool;
        /// When set, the page fault was caused by a write access. When not set,
        /// it was caused by a read access.
        pub const WRITE: bool;
        /// When set, the page fault was caused while CPL = 3. This does not
        /// necessarily mean that the page fault was a privilege violation.
        pub const USER: bool;
        /// When set, one or more page directory entries contain reserved bits
        /// which are set to 1. This only applies when the PSE or PAE flags in
        /// CR4 are set to 1.
        pub const RESERVED_WRITE: bool;
        /// When set, the page fault was caused by an instruction fetch. This
        /// only applies when the No-Execute bit is supported and enabled.
        pub const INSTRUCTION_FETCH: bool;
        /// When set, the page fault was caused by a protection-key violation.
        /// The PKRU register (for user-mode accesses) or PKRS MSR (for
        /// supervisor-mode accesses) specifies the protection key rights.
        pub const PROTECTION_KEY: bool;
        /// When set, the page fault was caused by a shadow stack access.
        pub const SHADOW_STACK: bool;
        const _RESERVED0 = 8;
        /// When set, the fault was due to an SGX violation. The fault is
        /// unrelated to ordinary paging.
        pub const SGX: bool;
    }
}

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

static IDT: Mutex<idt::Idt, Spinlock> = Mutex::new_with_raw_mutex(idt::Idt::new(), Spinlock::new());
static INTERRUPT_CONTROLLER: InitOnce<Controller> = InitOnce::uninitialized();

/// ISA interrupt vectors
///
/// See: [the other wiki](https://wiki.osdev.org/Interrupts#General_IBM-PC_Compatible_Interrupt_Information)
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum IsaInterrupt {
    /// Programmable Interval Timer (PIT) timer interrupt.
    PitTimer = 0,
    /// PS/2 keyboard controller interrupt.
    Ps2Keyboard = 1,
    // IRQ 2 is reserved for the PIC cascade interrupt and isn't user accessible!
    /// COM2 / COM4 serial port interrupt.
    Com2 = 3,
    /// COM1 / COM3 serial port interrupt.
    Com1 = 4,
    /// LPT2 parallel port interrupt.
    Lpt2 = 5,
    /// Floppy disk
    Floppy = 6,
    /// LPT1 parallel port interrupt, or spurious.
    Lpt1 = 7,
    /// CMOS real-time clock.
    CmosRtc = 8,
    /// Free for peripherals/SCSI/NIC
    Periph1 = 9,
    Periph2 = 10,
    Periph3 = 11,
    /// PS/2 Mouse
    Ps2Mouse = 12,
    /// FPU
    Fpu = 13,
    /// Primary ATA hard disk
    AtaPrimary = 14,
    /// Secondary ATA hard disk
    AtaSecondary = 15,
}

impl IsaInterrupt {
    pub const ALL: [IsaInterrupt; 15] = [
        IsaInterrupt::PitTimer,
        IsaInterrupt::Ps2Keyboard,
        IsaInterrupt::Com2,
        IsaInterrupt::Com1,
        IsaInterrupt::Lpt2,
        IsaInterrupt::Floppy,
        IsaInterrupt::Lpt1,
        IsaInterrupt::CmosRtc,
        IsaInterrupt::Periph1,
        IsaInterrupt::Periph2,
        IsaInterrupt::Periph3,
        IsaInterrupt::Ps2Mouse,
        IsaInterrupt::Fpu,
        IsaInterrupt::AtaPrimary,
        IsaInterrupt::AtaSecondary,
    ];
}

#[must_use]
fn disable_scoped() -> impl Drop + Send + Sync {
    unsafe {
        crate::cpu::intrinsics::cli();
    }
    mycelium_util::defer(|| unsafe {
        crate::cpu::intrinsics::sti();
    })
}

impl Controller {
    // const DEFAULT_IOAPIC_BASE_PADDR: u64 = 0xFEC00000;

    pub fn idt() -> MutexGuard<'static, idt::Idt, Spinlock> {
        IDT.lock()
    }

    #[tracing::instrument(level = "info", name = "interrupt::Controller::init")]
    pub fn init<H: Handlers<Registers>>() {
        tracing::info!("intializing IDT...");

        let mut idt = IDT.lock();
        idt.register_handlers::<H>().unwrap();
        unsafe {
            idt.load_raw();
        }
    }

    pub fn mask_isa_irq(&self, irq: IsaInterrupt) {
        match self.model {
            InterruptModel::Pic(ref pics) => pics.lock().mask(irq),
            InterruptModel::Apic { ref io, .. } => io.set_isa_masked(irq, true),
        }
    }

    pub fn unmask_isa_irq(&self, irq: IsaInterrupt) {
        match self.model {
            InterruptModel::Pic(ref pics) => pics.lock().unmask(irq),
            InterruptModel::Apic { ref io, .. } => io.set_isa_masked(irq, false),
        }
    }

    fn local_apic_handle(&self) -> Result<&apic::local::Handle, apic::local::LocalApicError> {
        match self.model {
            InterruptModel::Pic(_) => Err(apic::local::LocalApicError::NoApic),
            InterruptModel::Apic { ref local, .. } => Ok(local),
        }
    }

    pub fn with_local_apic<T>(
        &self,
        f: impl FnOnce(&LocalApic) -> T,
    ) -> Result<T, apic::local::LocalApicError> {
        self.local_apic_handle()?.with(f)
    }

    /// This should *not* be called by the boot processor
    pub fn initialize_local_apic<A>(
        &self,
        frame_alloc: &A,
        pagectrl: &mut impl page::Map<mm::size::Size4Kb, A>,
    ) -> Result<(), apic::local::LocalApicError>
    where
        A: page::Alloc<mm::size::Size4Kb>,
    {
        let _deferred = disable_scoped();
        let hdl = self.local_apic_handle()?;
        unsafe {
            hdl.initialize(frame_alloc, pagectrl, Idt::LOCAL_APIC_SPURIOUS as u8);
        }
        Ok(())
    }
    /// # Safety
    ///
    /// Calling this when there isn't actually an ISA interrupt pending can do
    /// arbitrary bad things (which I think is basically just faulting the CPU).
    pub unsafe fn end_isa_irq(&self, irq: IsaInterrupt) {
        match self.model {
            InterruptModel::Pic(ref pics) => pics.lock().end_interrupt(irq),
            InterruptModel::Apic { ref local, .. } => local.with(|apic| unsafe { apic.end_interrupt() })
                .expect("interrupts should not be handled on this core until the local APIC is initialized")
        }
    }

    pub fn enable_hardware_interrupts(
        acpi: Option<&acpi::InterruptModel>,
        frame_alloc: &impl page::Alloc<mm::size::Size4Kb>,
    ) -> &'static Self {
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

        let controller = match acpi {
            Some(acpi::InterruptModel::Apic(apic_info)) => {
                tracing::info!("detected APIC interrupt model");

                let mut pagectrl = mm::PageCtrl::current();

                // disable the 8259 PICs so that we can use APIC interrupts instead
                unsafe {
                    pics.disable();
                }
                tracing::info!("disabled 8259 PICs");

                // configure the I/O APIC(s)
                let io = IoApicSet::new(apic_info, frame_alloc, &mut pagectrl, Idt::ISA_BASE as u8);

                // configure and initialize the local APIC on the boot processor
                let local = apic::local::Handle::new();
                unsafe {
                    local.initialize(frame_alloc, &mut pagectrl, Idt::LOCAL_APIC_SPURIOUS as u8);
                }

                let model = InterruptModel::Apic { local, io };

                tracing::trace!(interrupt_model = ?model);

                INTERRUPT_CONTROLLER.init(Self { model })
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
                INTERRUPT_CONTROLLER.init(Self {
                    model: InterruptModel::Pic(Mutex::new_with_raw_mutex(pics, Spinlock::new())),
                })
            }
        };

        unsafe {
            crate::cpu::intrinsics::sti();
        }

        // There's a weird behavior in QEMU where, apparently, when we unmask
        // the PIT interrupt, it might fire spuriously *as soon as its
        // unmasked*, even if we haven't actually done an `sti`. I don't get
        // this and it seems wrong, but it seems to happen occasionally with the
        // default QEMU acceleration, and pretty consistently with `-machine
        // accel=kvm`, so *maybe* it can also happen on a real computer?
        //
        // Anyway, because of this, we can't unmask the PIT interrupt until
        // after we've actually initialized the interrupt controller static.
        // Otherwise, if we unmask it before initializing the static (like we
        // used to), the interrupt gets raised immediately, and when the ISR
        // tries to actually send an EOI to ack it, it dereferences
        // uninitialized memory and the kernel just hangs. So, we wait to do it
        // until here.
        //
        // The fact that this behavior exists at all makes me really, profoundly
        // uncomfortable: shouldn't `cli` like, actually do what it's supposed
        // to? But, we can just choose to unmask the PIT later and it's fine, I
        // guess...
        controller.unmask_isa_irq(IsaInterrupt::PitTimer);
        controller.unmask_isa_irq(IsaInterrupt::Ps2Keyboard);
        controller
    }

    /// Starts a periodic timer which fires the `timer_tick` interrupt of the
    /// provided [`Handlers`] every time `interval` elapses.
    pub fn start_periodic_timer(&self, interval: Duration) -> Result<(), PeriodicTimerError> {
        match self.model {
            InterruptModel::Pic(_) => crate::time::PIT
                .lock()
                .start_periodic_timer(interval)
                .map_err(Into::into),
            InterruptModel::Apic { ref local, .. } => local.with(|apic| {
                // divide by 16 is chosen kinda arbitrarily lol
                apic.calibrate_timer(apic::local::register::TimerDivisor::By16);
                apic.start_periodic_timer(interval, Idt::LOCAL_APIC_TIMER as u8)?;
                Ok(())
            })?,
        }
    }
}

impl<T> hal_core::interrupt::Context for Context<'_, T> {
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

impl ctx::PageFault for Context<'_, PageFaultCode> {
    fn fault_vaddr(&self) -> crate::VAddr {
        crate::control_regs::Cr2::read()
    }

    fn debug_error_code(&self) -> &dyn fmt::Debug {
        &self.code
    }

    fn display_error_code(&self) -> &dyn fmt::Display {
        &self.code
    }
}

impl ctx::CodeFault for Context<'_, CodeFault<'_>> {
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

impl Context<'_, ErrorCode> {
    pub fn error_code(&self) -> ErrorCode {
        self.code
    }
}

impl Context<'_, PageFaultCode> {
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
        let span = tracing::debug_span!("Idt::register_handlers");
        let _enter = span.enter();

        // === exceptions ===
        // these exceptions are mapped to the HAL `Handlers` trait's "code
        // fault" handler, and indicate that the code that was executing did a
        // Bad Thing
        self.register_isr(Self::DIVIDE_BY_ZERO, isr::div_0::<H> as *const ());
        self.register_isr(Self::OVERFLOW, isr::overflow::<H> as *const ());
        self.register_isr(Self::BOUND_RANGE_EXCEEDED, isr::br::<H> as *const ());
        self.register_isr(Self::INVALID_OPCODE, isr::ud::<H> as *const ());
        self.register_isr(Self::DEVICE_NOT_AVAILABLE, isr::no_fpu::<H> as *const ());
        self.register_isr(
            Self::ALIGNMENT_CHECK,
            isr::alignment_check::<H> as *const (),
        );
        self.register_isr(
            Self::SIMD_FLOATING_POINT,
            isr::simd_fp_exn::<H> as *const (),
        );
        self.register_isr(Self::X87_FPU_EXCEPTION, isr::x87_exn::<H> as *const ());

        // other exceptions, not mapped to the "code fault" handler
        self.register_isr(Self::PAGE_FAULT, isr::page_fault::<H> as *const ());
        self.register_isr(Self::INVALID_TSS, isr::invalid_tss::<H> as *const ());
        self.register_isr(
            Self::SEGMENT_NOT_PRESENT,
            isr::segment_not_present::<H> as *const (),
        );
        self.register_isr(
            Self::STACK_SEGMENT_FAULT,
            isr::stack_segment::<H> as *const (),
        );
        self.register_isr(Self::GENERAL_PROTECTION_FAULT, isr::gpf::<H> as *const ());
        self.register_isr(Self::DOUBLE_FAULT, isr::double_fault::<H> as *const ());

        // === hardware interrupts ===
        // ISA standard hardware interrupts mapped on both the PICs and IO APIC
        // interrupt models.
        self.register_isa_isr(IsaInterrupt::PitTimer, isr::pit_timer::<H> as *const ());
        self.register_isa_isr(IsaInterrupt::Ps2Keyboard, isr::keyboard::<H> as *const ());

        // local APIC specific hardware interrupts
        self.register_isr(Self::LOCAL_APIC_SPURIOUS, isr::spurious as *const ());
        self.register_isr(Self::LOCAL_APIC_TIMER, isr::apic_timer::<H> as *const ());

        // vector 69 (nice) is reserved by the HAL for testing the IDT.
        self.register_isr(69, isr::test::<H> as *const ());

        Ok(())
    }
}

/// Forcefully unlock the VGA port and COM1 serial port (used by tracing), so
/// that an ISR can log stuff without deadlocking.
///
/// # Safety
///
/// This forcefully unlocks a mutex, which is probably bad to do. Only do this
/// in ISRs that definitely represent real actual faults, and not just because
/// "you wanted to log something".
unsafe fn force_unlock_tracing() {
    crate::vga::writer().force_unlock();
    if let Some(com1) = crate::serial::com1() {
        com1.force_unlock();
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

mod isr {
    use super::*;

    macro_rules! gen_code_faults {
        ($(fn $name:ident($($rest:tt)+),)+) => {
            $(
                gen_code_faults! {@ $name($($rest)+); }
            )+
        };
        (@ $name:ident($kind:literal);) => {
            pub(super) extern "x86-interrupt" fn $name<H: Handlers<Registers>>(mut registers: Registers) {
                let code = CodeFault {
                    error_code: None,
                    kind: $kind,
                };
                H::code_fault(Context { registers: &mut registers, code });
            }
        };
        (@ $name:ident($kind:literal, code);) => {
            pub(super) extern "x86-interrupt" fn $name<H: Handlers<Registers>>(
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

    gen_code_faults! {
        fn div_0("Divide-By-Zero (0x0)"),
        fn overflow("Overflow (0x4)"),
        fn br("Bound Range Exceeded (0x5)"),
        fn ud("Invalid Opcode (0x6)"),
        fn no_fpu("Device (FPU) Not Available (0x7)"),
        fn alignment_check("Alignment Check (0x11)", code),
        fn simd_fp_exn("SIMD Floating-Point Exception (0x13)"),
        fn x87_exn("x87 Floating-Point Exception (0x10)"),
    }

    pub(super) extern "x86-interrupt" fn page_fault<H: Handlers<Registers>>(
        mut registers: Registers,
        code: PageFaultCode,
    ) {
        H::page_fault(Context {
            registers: &mut registers,
            code,
        });
    }

    pub(super) extern "x86-interrupt" fn double_fault<H: Handlers<Registers>>(
        mut registers: Registers,
        code: u64,
    ) {
        H::double_fault(Context {
            registers: &mut registers,
            code,
        });
    }

    pub(super) extern "x86-interrupt" fn pit_timer<H: Handlers<Registers>>(_regs: Registers) {
        if crate::time::Pit::handle_interrupt() {
            H::timer_tick()
        }
        unsafe {
            INTERRUPT_CONTROLLER
                .get_unchecked()
                .end_isa_irq(IsaInterrupt::PitTimer);
        }
    }

    pub(super) extern "x86-interrupt" fn apic_timer<H: Handlers<Registers>>(_regs: Registers) {
        H::timer_tick();
        unsafe {
            match INTERRUPT_CONTROLLER.get_unchecked().model {
                InterruptModel::Pic(_) => unreachable!(),
                InterruptModel::Apic { ref local, .. } => {
                    match local.with(|apic| apic.end_interrupt()) {
                        Ok(_) => {}
                        Err(e) => unreachable!(
                            "the local APIC timer will not have fired if the \
                             local APIC is uninitialized on this core! {e:?}",
                        ),
                    }
                }
            }
        }
    }

    pub(super) extern "x86-interrupt" fn keyboard<H: Handlers<Registers>>(_regs: Registers) {
        // 0x60 is a magic PC/AT number.
        static PORT: cpu::Port = cpu::Port::at(0x60);
        // load-bearing read - if we don't read from the keyboard controller it won't
        // send another interrupt on later keystrokes.
        let scancode = unsafe { PORT.readb() };
        H::ps2_keyboard(scancode);
        unsafe {
            INTERRUPT_CONTROLLER
                .get_unchecked()
                .end_isa_irq(IsaInterrupt::Ps2Keyboard);
        }
    }

    pub(super) extern "x86-interrupt" fn test<H: Handlers<Registers>>(mut registers: Registers) {
        H::test_interrupt(Context {
            registers: &mut registers,
            code: (),
        });
    }

    pub(super) extern "x86-interrupt" fn invalid_tss<H: Handlers<Registers>>(
        mut registers: Registers,
        code: u64,
    ) {
        unsafe {
            // Safety: An invalid TSS exception is always an oops. Since we're
            // not coming back from this, it's okay to forcefully unlock the
            // tracing outputs.
            force_unlock_tracing();
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

    pub(super) extern "x86-interrupt" fn segment_not_present<H: Handlers<Registers>>(
        mut registers: Registers,
        code: u64,
    ) {
        unsafe {
            // Safety: An segment not present exception is always an oops.
            // Since we're not coming back from this, it's okay to
            // forcefully unlock the tracing outputs.
            force_unlock_tracing();
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

    pub(super) extern "x86-interrupt" fn stack_segment<H: Handlers<Registers>>(
        mut registers: Registers,
        code: u64,
    ) {
        unsafe {
            // Safety: An stack-segment fault exeption is always an oops.
            // Since we're not coming back from this, it's okay to
            // forcefully unlock the tracing outputs.
            force_unlock_tracing();
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

    pub(super) extern "x86-interrupt" fn gpf<H: Handlers<Registers>>(
        mut registers: Registers,
        code: u64,
    ) {
        unsafe {
            // Safety: A general protection fault is (currently) always an
            // oops. Since we're not coming back from this, it's okay to
            // forcefully unlock the tracing outputs.
            //
            // TODO(eliza): in the future, if we allow the kernel to
            // recover from general protection faults in user mode programs,
            // rather than treating them as invariably fatal, we should
            // probably not always do this. Instead, we should just handle
            // the user-mode GPF non-fatally, and only unlock the tracing
            // stuff if we know we're going to do a kernel oops...
            force_unlock_tracing();
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

    pub(super) extern "x86-interrupt" fn spurious() {
        // TODO(eliza): do we need to actually do something here?
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
