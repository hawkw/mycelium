use hal_x86_64::segment::{self, Gdt};
use mycelium_util::spin::Mutex;

pub(super) static GDT: Mutex<Gdt<8>> = Mutex::new(Gdt::new());

#[tracing::instrument(level = tracing::Level::DEBUG)]
pub(super) fn init_gdt() {
    tracing::trace!("initializing GDT...");
    let mut gdt = GDT.lock();

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
