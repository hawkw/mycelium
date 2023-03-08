use super::{oops, segmentation, Oops};
use core::sync::atomic::{AtomicUsize, Ordering};
use hal_core::interrupt;
pub use hal_x86_64::interrupt::*;
use maitake::time;

#[tracing::instrument]
pub fn enable_exceptions() {
    segmentation::init();
    tracing::info!("GDT initialized!");

    Controller::init::<InterruptHandlers>();
    tracing::info!("IDT initialized!");
}

#[tracing::instrument(skip(acpi))]
pub fn enable_hardware_interrupts(acpi: Option<&acpi::InterruptModel>) {
    let controller = Controller::enable_hardware_interrupts(acpi, &crate::ALLOC);
    controller
        .start_periodic_timer(TIMER_INTERVAL)
        .expect("10ms should be a reasonable interval for the PIT or local APIC timer...");
    tracing::info!(granularity = ?TIMER_INTERVAL, "global timer initialized")
}

pub const TIMER_INTERVAL: time::Duration = time::Duration::from_millis(10);

static TEST_INTERRUPT_WAS_FIRED: AtomicUsize = AtomicUsize::new(0);

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
        crate::rt::TIMER.pend_ticks(1);
    }

    fn ps2_keyboard(scancode: u8) {
        crate::drivers::ps2_keyboard::handle_scancode(scancode)
    }

    fn test_interrupt<C>(cx: C)
    where
        C: hal_core::interrupt::ctx::Context<Registers = Registers>,
    {
        let fired = TEST_INTERRUPT_WAS_FIRED.fetch_add(1, Ordering::Release) + 1;
        tracing::info!(registers = ?cx.registers(), fired, "lol im in ur test interrupt");
    }
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
