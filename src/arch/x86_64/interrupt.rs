use hal_core::VAddr;

pub fn init_interrupts() {
    init_gdt();
}

// TODO(eliza): put this somewhere good.
type StackFrame = [u8; 4096];

const DOUBLE_FAULT_IST: usize = 7;

#[inline]
fn init_gdt() {
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

    static TSS: task::StateSegment = {
        let mut tss = task::StateSegment::empty();
        tss.interrupt_stacks[DOUBLE_FAULT_IST] = unsafe {
            // safety: asdf
            VAddr::from_usize_unchecked(&DOUBLE_FAULT_STACK as *const _ as usize)
        };
        tss
    };

    static GDT: Gdt = {
        let mut gdt = Gdt::new();
        // add one kernel code segment
        gdt.add_user_segment(segment::Descriptor::code().with_ring(Ring::Ring0));
        // add the TSS.
        gdt.add_sys_segment(segment::SystemDescriptor::tss(&TSS));
        // all done! long mode barely uses this thing lol.
        gdt
    };

    gdt.load();
}
