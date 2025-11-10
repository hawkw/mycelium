use super::{oops, Oops};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use hal_core::{interrupt, VAddr};
pub use hal_x86_64::interrupt::*;
use hal_x86_64::{
    cpu::Ring,
    segment::{self, Gdt},
    task,
};
use maitake::time;
use mycelium_util::{fmt, sync};

#[tracing::instrument]
pub fn enable_exceptions() {
    init_gdt();
    tracing::info!("GDT initialized!");

    Controller::init::<InterruptHandlers>();
    tracing::info!("IDT initialized!");
}

#[tracing::instrument(skip(acpi))]
pub fn enable_hardware_interrupts(acpi: Option<&acpi::InterruptModel>) -> &'static Controller {
    let irq_ctrl = Controller::enable_hardware_interrupts(acpi, &crate::ALLOC);

    irq_ctrl
        .start_periodic_timer(IDIOTIC_CLOCK_INTERVAL)
        .expect("failed to start periodic timer");
    irq_ctrl
}

// TODO(eliza): put this somewhere good.
type StackFrame = [u8; 4096];

// chosen by fair dice roll, guaranteed to be random
const DOUBLE_FAULT_STACK_SIZE: usize = 8;

/// Stack used by ISRs during a double fault.
///
/// /!\ EXTREMELY SERIOUS WARNING: this has to be `static mut` or else it
///     will go in `.bss` and we'll all die or something.
static mut DOUBLE_FAULT_STACK: [StackFrame; DOUBLE_FAULT_STACK_SIZE] =
    [[0; 4096]; DOUBLE_FAULT_STACK_SIZE];

static TSS: sync::Lazy<task::StateSegment> = sync::Lazy::new(|| {
    tracing::trace!("initializing TSS..");
    let mut tss = task::StateSegment::empty();
    tss.interrupt_stacks[Idt::DOUBLE_FAULT_IST_OFFSET] =
        VAddr::from_ptr(core::ptr::addr_of!(DOUBLE_FAULT_STACK))
            .offset(DOUBLE_FAULT_STACK_SIZE as isize);
    tracing::debug!(?tss, "TSS initialized");
    tss
});

pub(in crate::arch) static GDT: sync::InitOnce<Gdt> = sync::InitOnce::uninitialized();

const IDIOTIC_CLOCK_INTERVAL: time::Duration = time::Duration::from_millis(10);

static IDIOTIC_CLOCK_TICKS: AtomicU64 = AtomicU64::new(0);

static TEST_INTERRUPT_WAS_FIRED: AtomicUsize = AtomicUsize::new(0);

pub const IDIOTIC_CLOCK: maitake::time::Clock =
    maitake::time::Clock::new(IDIOTIC_CLOCK_INTERVAL, || {
        IDIOTIC_CLOCK_TICKS.load(Ordering::Relaxed)
    })
    .named("CLOCK_IDIOTIC");

pub(crate) struct InterruptHandlers;

/// Forcibly unlock the IOs we write to in an oops (VGA buffer and COM1 serial
/// port) to prevent deadlocks if the oops occured while either was locked.
///
/// # Safety
///
///  /!\ only call this when oopsing!!! /!\
impl hal_core::interrupt::Handlers<Registers> for InterruptHandlers {
    fn page_fault<C>(cx: C)
    where
        C: interrupt::Context<Registers = Registers> + hal_core::interrupt::ctx::PageFault,
    {
        let fault_vaddr = cx.fault_vaddr();
        let code = cx.display_error_code();
        oops(Oops::fault_with_details(
            &cx,
            "PAGE FAULT",
            &format_args!("at {fault_vaddr:?}\n{code}"),
        ))
    }

    fn code_fault<C>(cx: C)
    where
        C: interrupt::Context<Registers = Registers> + interrupt::ctx::CodeFault,
    {
        let fault = match cx.details() {
            Some(deets) => Oops::fault_with_details(&cx, cx.fault_kind(), deets),
            None => Oops::fault(&cx, cx.fault_kind()),
        };
        oops(fault)
    }

    fn double_fault<C>(cx: C)
    where
        C: hal_core::interrupt::Context<Registers = Registers>,
    {
        oops(Oops::fault(&cx, "DOUBLE FAULT"))
    }

    fn timer_tick() {
        IDIOTIC_CLOCK_TICKS.fetch_add(1, Ordering::Release);
    }

    fn ps2_keyboard(scancode: u8) {
        crate::drivers::ps2_keyboard::handle_scancode(scancode)
    }

    fn serial_input(port: u8, byte: u8) {
        crate::drivers::serial_input::SERIAL_INPUTS.get()[port as usize].handle_input(byte)
    }

    fn test_interrupt<C>(cx: C)
    where
        C: hal_core::interrupt::ctx::Context<Registers = Registers>,
    {
        let fired = TEST_INTERRUPT_WAS_FIRED.fetch_add(1, Ordering::Release) + 1;
        tracing::info!(registers = ?cx.registers(), fired, "lol im in ur test interrupt");
    }
}

#[inline]
#[tracing::instrument(level = tracing::Level::DEBUG)]
pub(super) fn init_gdt() {
    tracing::trace!("initializing GDT...");
    let mut gdt = Gdt::new();

    // add one kernel code segment
    let code_segment = segment::Descriptor::code().with_ring(Ring::Ring0);
    let code_selector = gdt.add_segment(code_segment);
    tracing::debug!(
        descriptor = fmt::alt(code_segment),
        selector = fmt::alt(code_selector),
        "added code segment"
    );

    // add the TSS.

    let tss = segment::SystemDescriptor::tss(&TSS);
    let tss_selector = gdt.add_sys_segment(tss);
    tracing::debug!(
        tss.descriptor = fmt::alt(tss),
        tss.selector = fmt::alt(tss_selector),
        "added TSS"
    );

    // all done! long mode barely uses this thing lol.
    GDT.init(gdt);

    // load the GDT
    let gdt = GDT.get();
    tracing::debug!(GDT = ?gdt, "GDT initialized");
    gdt.load();

    tracing::trace!("GDT loaded");

    // set new segment selectors
    let code_selector = segment::Selector::current_cs();
    tracing::trace!(code_selector = fmt::alt(code_selector));
    unsafe {
        // set the code segment selector
        code_selector.set_cs();

        // in protected mode and long mode, the code segment, stack segment,
        // data segment, and extra segment must all have base address 0 and
        // limit `2^64`, since actual segmentation is not used in those modes.
        // therefore, we must zero the SS, DS, and ES registers.
        segment::Selector::null().set_ss();
        segment::Selector::null().set_ds();
        segment::Selector::null().set_es();

        task::StateSegment::load_tss(tss_selector);
    }

    tracing::debug!("segment selectors set");
}

mycotest::decl_test! {
    fn interrupts_work() -> mycotest::TestResult {
        let test_interrupt_fires = TEST_INTERRUPT_WAS_FIRED.load(Ordering::Acquire);

        tracing::debug!("testing interrupts...");
        fire_test_interrupt();
        tracing::debug!("it worked");

        mycotest::assert_eq!(
            test_interrupt_fires + 1,
            TEST_INTERRUPT_WAS_FIRED.load(Ordering::Acquire),
            "test interrupt wasn't fired!",
        );

        Ok(())
    }
}
