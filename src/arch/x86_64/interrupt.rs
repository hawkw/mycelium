use hal_core::VAddr;
pub use hal_x86_64::interrupt::*;

// TODO(eliza): put this somewhere good.
type StackFrame = [u8; 4096];

const DOUBLE_FAULT_IST: usize = 6;

#[inline]
pub(super) fn init_gdt() {
    use hal_x86_64::{
        cpu::Ring,
        segment::{self, Gdt},
        task,
    };
    use mycelium_util::sync::Lazy;

    // chosen by fair dice roll, guaranteed to be random
    const DOUBLE_FAULT_STACK_SIZE: usize = 8;

    /// Stack used by ISRs during a double fault.
    ///
    /// /!\ EXTREMELY SERIOUS WARNING: this has to be `static mut` or else it
    ///     will go in `.bss` and we'll all die or something.
    static mut DOUBLE_FAULT_STACK: [StackFrame; DOUBLE_FAULT_STACK_SIZE] =
        [[0; 4096]; DOUBLE_FAULT_STACK_SIZE];

    static TSS: Lazy<task::StateSegment> = Lazy::new(|| {
        tracing::trace!("initializing TSS..");
        let mut tss = task::StateSegment::empty();
        tss.interrupt_stacks[DOUBLE_FAULT_IST] = unsafe {
            // safety: asdf
            VAddr::from_usize_unchecked(&DOUBLE_FAULT_STACK as *const _ as usize)
        };
        tracing::trace!(?tss);
        tss
    });

    static GDT: Lazy<Gdt> = Lazy::new(|| {
        tracing::trace!("initializing GDT...");
        let mut gdt = Gdt::new();
        // add one kernel code segment
        let code_segment = segment::Descriptor::code().with_ring(Ring::Ring0);
        gdt.add_user_segment(code_segment);
        tracing::trace!("added code segment: {:?}", code_segment);
        // add the TSS.
        let tss = segment::SystemDescriptor::tss(&TSS);
        gdt.add_sys_segment(tss);
        tracing::trace!("added TSS: {:?}", tss);
        // all done! long mode barely uses this thing lol.
        gdt
    });

    GDT.load();
}
