use hal_core::VAddr;
pub use hal_x86_64::interrupt::*;
use hal_x86_64::{
    cpu::Ring,
    segment::{self, Gdt},
    task,
};
use mycelium_util::{fmt, sync};

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
    tss.interrupt_stacks[Idt::DOUBLE_FAULT_IST_OFFSET] = unsafe {
        // safety: asdf
        VAddr::of(&DOUBLE_FAULT_STACK).offset(DOUBLE_FAULT_STACK_SIZE as i32)
    };
    tracing::debug!(?tss, "TSS initialized");
    tss
});

static GDT: sync::InitOnce<Gdt> = sync::InitOnce::uninitialized();

#[inline]
#[tracing::instrument(level = "debug")]
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
