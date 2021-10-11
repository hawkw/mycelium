use hal_core::VAddr;
pub use hal_x86_64::interrupt::*;
use mycelium_util::{fmt, sync};

// TODO(eliza): put this somewhere good.
type StackFrame = [u8; 4096];

const DOUBLE_FAULT_IST: usize = 6;

#[inline]
#[tracing::instrument(level = "debug")]
pub(super) fn init_gdt() {
    use hal_x86_64::{
        cpu::Ring,
        segment::{self, Gdt},
        task,
    };

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
        tss.interrupt_stacks[DOUBLE_FAULT_IST] = unsafe {
            // safety: asdf
            VAddr::of(&DOUBLE_FAULT_STACK[7][4095])
        };
        tracing::debug!(?tss, "TSS initialized");
        tss
    });

    static GDT: sync::InitOnce<Gdt> = sync::InitOnce::uninitialized();

    tracing::trace!("initializing GDT...");
    let mut gdt = Gdt::new();

    // add one kernel code segment
    let code_segment = segment::Descriptor::code().with_ring(Ring::Ring0);
    let code_selector = gdt.add_user_segment(code_segment);
    tracing::trace!(
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
    tracing::debug!(?GDT, "GDT initialized");

    // load the GDT
    GDT.get().load();

    tracing::trace!("GDT loaded");

    // set new segment selectors
    let code_selector = segment::Selector::cs();
    tracing::trace!(code_selector = fmt::alt(code_selector));
    unsafe {
        code_selector.set_cs();
        tracing::trace!("setting tss segment...");
        task::StateSegment::load_tss(tss_selector);
    }

    tracing::debug!("segment selectors set");
}
