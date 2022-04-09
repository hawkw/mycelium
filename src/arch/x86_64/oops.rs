use super::framebuf;
use core::{
    panic::PanicInfo,
    sync::atomic::{AtomicBool, Ordering},
};
use embedded_graphics::{
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::{Rgb888, RgbColor as _},
    prelude::*,
    text::{Alignment, Text},
};
use hal_core::{
    framebuffer::{Draw, RgbColor},
    interrupt, Address,
};
use hal_x86_64::{cpu, interrupt::Registers as X64Registers, serial, vga};
use mycelium_trace::{embedded_graphics::MakeTextWriter, writer::MakeWriter};
use mycelium_util::fmt::{self, Write};

#[derive(Debug)]
pub struct Oops<'a> {
    already_panicked: bool,
    already_faulted: bool,
    situation: OopsSituation<'a>,
}

enum OopsSituation<'a> {
    Fault {
        kind: &'static str,
        fault: Fault<'a>,
        details: Option<&'a dyn fmt::Display>,
    },
    Panic(&'a PanicInfo<'a>),
}

type Fault<'a> = &'a dyn interrupt::ctx::Context<Registers = X64Registers>;

#[cold]
pub fn oops(oops: Oops<'_>) -> ! {
    let mut vga = vga::writer();

    // /!\ disable all interrupts, unlock everything to prevent deadlock /!\
    //
    // Safety: it is okay to do this because we are oopsing and everything
    // is going to die anyway.
    unsafe {
        // disable all interrupts.
        cpu::intrinsics::cli();

        // If the system has a COM1, unlock it.
        if let Some(com1) = serial::com1() {
            com1.force_unlock();
        }

        // unlock the VGA buffer.
        vga.force_unlock();

        // unlock the frame buffer
        framebuf::force_unlock();
    }

    // emit a DEBUG event first. with the default tracing configuration, these
    // will go to the serial port and *not* get written to the framebuffer. this
    // is important, since it lets us still get information about the oops on
    // the serial port, even if the oops was due to a bug in eliza's framebuffer
    // code, which (it turns out) is surprisingly janky...
    tracing::debug!(?oops);
    // okay, we've dumped the oops to serial, now try to log a nicer event at
    // the ERROR level.
    // let registers = if let Some(fault) = fault {
    //     let registers = fault.registers();
    //     tracing::error!(%cause, ?registers, "OOPS! a CPU fault occurred");
    //     Some(registers)
    // } else {
    //     tracing::error!(%cause, "OOPS! a panic occurred", );
    //     None
    // };

    let mut framebuf = unsafe { super::framebuf::mk_framebuf() };
    framebuf.fill(RgbColor::RED);

    let mut target = framebuf.as_draw_target();
    let smol = MonoTextStyleBuilder::new()
        .font(&ascii::FONT_6X10)
        .text_color(Rgb888::WHITE)
        .build();
    let uwu = MonoTextStyleBuilder::new()
        .font(&ascii::FONT_9X15_BOLD)
        .text_color(Rgb888::RED)
        .background_color(Rgb888::WHITE)
        .build();

    let _ = Text::with_alignment(
        oops.situation.header(),
        Point::new(5, 15),
        uwu,
        Alignment::Left,
    )
    .draw(&mut target)
    .unwrap();
    let _ = Text::with_alignment(
        "uwu mycelium did a widdle fucky-wucky!",
        Point::new(5, 30),
        smol,
        Alignment::Left,
    )
    .draw(&mut target)
    .unwrap();
    drop(target);
    drop(framebuf);

    let mk_writer = MakeTextWriter::new_at(
        || unsafe { super::framebuf::mk_framebuf() },
        Point::new(4, 45),
    );

    match oops.situation {
        OopsSituation::Fault {
            kind,
            details: Some(deets),
            ..
        } => writeln!(mk_writer.make_writer(), "a {} occurred: {}\n", kind, deets).unwrap(),
        OopsSituation::Fault {
            kind,
            details: None,
            ..
        } => writeln!(mk_writer.make_writer(), "a {} occurred!\n", kind).unwrap(),
        OopsSituation::Panic(panic) => {
            let mut writer = mk_writer.make_writer();
            match panic.message() {
                Some(msg) => writeln!(writer, "mycelium panicked: {}", msg).unwrap(),
                None => writeln!(writer, "mycelium panicked!").unwrap(),
            }
            if let Some(loc) = panic.location() {
                writeln!(writer, "at {}:{}:{}", loc.file(), loc.line(), loc.column()).unwrap();
            }
        }
    }

    if let Some(registers) = oops.situation.registers() {
        {
            let mut writer = mk_writer.make_writer();
            writeln!(
                writer,
                "%rip    = {:#016x}",
                registers.instruction_ptr.as_usize()
            )
            .unwrap();
            writeln!(writer, "%rsp    = {:#016x}", registers.stack_ptr.as_usize()).unwrap();
            writeln!(writer, "%cs     = {:016x}", registers.code_segment).unwrap();
            writeln!(writer, "%ss     = {:016x}", registers.stack_segment).unwrap();
            writeln!(writer, "%rflags = {:#016b}", registers.cpu_flags).unwrap();
        }

        tracing::debug!(?registers.instruction_ptr);
        tracing::debug!(?registers.stack_ptr);
        tracing::debug!(?registers.code_segment);
        tracing::debug!(?registers.stack_segment);
        tracing::debug!(registers.cpu_flags = ?fmt::bin(registers.cpu_flags));
        // TODO(eliza): disassembly appears to (always?) do a general protection
        // fault. seems weird.
        /*
        // skip printing disassembly if we already faulted; disassembling the
        // fault address may fault a second time.
        if !oops.already_faulted {
            let fault_addr = registers.instruction_ptr.as_usize();
            disassembly(fault_addr);
        }
        */
    }

    crate::ALLOC.dump_free_lists();

    if oops.already_panicked {
        writeln!(
            mk_writer.make_writer(),
            "...while handling a panic! we really screwed up!"
        )
        .unwrap();
    }

    if oops.already_faulted {
        if oops.already_panicked {
            writeln!(
                mk_writer.make_writer(),
                "...while handling a fault! seems real bad lol!"
            )
            .unwrap();
        }
    }

    writeln!(
        mk_writer.make_writer(),
        "\nit will NEVER be safe to turn off your computer!"
    )
    .unwrap();

    #[cfg(test)]
    oops.fail_test();

    #[cfg(not(test))]
    unsafe {
        cpu::halt()
    }
}

// === impl Oops ===

static IS_PANICKING: AtomicBool = AtomicBool::new(false);
static IS_FAULTING: AtomicBool = AtomicBool::new(false);

impl<'a> Oops<'a> {
    #[inline(always)] // don't push a stack frame in case we overflowed!
    pub(super) fn fault(fault: Fault<'a>, kind: &'static str) -> Self {
        let situation = OopsSituation::Fault {
            kind,
            fault,
            details: None,
        };
        Self::mk_fault(situation)
    }

    #[inline(always)] // don't push a stack frame in case we overflowed!
    pub(super) fn fault_with_details(
        fault: Fault<'a>,
        kind: &'static str,
        details: &'a dyn fmt::Display,
    ) -> Self {
        let situation = OopsSituation::Fault {
            kind,
            fault,
            details: Some(details),
        };
        Self::mk_fault(situation)
    }

    #[inline(always)]
    fn mk_fault(situation: OopsSituation<'a>) -> Self {
        let already_panicked = IS_PANICKING.load(Ordering::Acquire);
        let already_faulted = IS_FAULTING.swap(true, Ordering::AcqRel);
        Self {
            already_panicked,
            already_faulted,
            situation,
        }
    }

    pub fn panic(panic: &'a PanicInfo<'a>) -> Self {
        let already_panicked = IS_PANICKING.swap(true, Ordering::AcqRel);
        let already_faulted = IS_FAULTING.load(Ordering::Acquire);
        Self {
            already_panicked,
            already_faulted,
            situation: OopsSituation::Panic(panic),
        }
    }

    #[cfg(test)]
    fn fail_test(&self) -> ! {
        use super::{qemu_exit, QemuExitCode};
        let failure = match self.situation {
            OopsSituation::Panic(_) => mycotest::Failure::Panic,
            OopsSituation::Fault { .. } => mycotest::Failure::Fault,
        };

        if let Some(test) = mycotest::runner::current_test() {
            if let Some(com1) = serial::com1() {
                // if writing the test outcome fails, don't double panic...
                let _ = test.write_outcome(Err(failure), com1.lock());
            }
        }
        qemu_exit(QemuExitCode::Failed)
    }
}

impl<'a> From<&'a PanicInfo<'a>> for Oops<'a> {
    #[inline]
    fn from(panic: &'a PanicInfo<'a>) -> Self {
        Self::panic(panic)
    }
}

// === impl OopsSituation ===

impl<'a> OopsSituation<'a> {
    fn header(&self) -> &'static str {
        match self {
            Self::Fault { .. } => " OOPSIE-WOOPSIE! ",
            Self::Panic(_) => " DON'T PANIC! ",
        }
    }

    fn registers(&self) -> Option<&X64Registers> {
        match self {
            Self::Fault { fault, .. } => Some(fault.registers()),
            _ => None,
        }
    }
}

impl fmt::Debug for OopsSituation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fault {
                kind,
                fault,
                details,
            } => {
                let mut dbg = f.debug_struct("OopsSituation::Fault");
                dbg.field("kind", kind)
                    .field("registers", fault.registers());
                if let Some(deets) = details {
                    dbg.field("details", &format_args!("\"{}\"", deets));
                }
                dbg.finish()
            }
            Self::Panic(panic) => f.debug_tuple("OopsSituation::Panic").field(&panic).finish(),
        }
    }
}

#[tracing::instrument(target = "oops", level = "error", skip(rip), fields(rip = fmt::hex(rip)))]
#[inline(always)]
fn disassembly(rip: usize) {
    use yaxpeax_arch::LengthedInstruction;
    // let _ = writeln!(mk_writer.make_writer(), "Disassembly:");
    let mut ptr = rip as u64;
    let decoder = yaxpeax_x86::long_mode::InstDecoder::default();
    for _ in 0..10 {
        // Safety: who cares! At worst this might double-fault by reading past the end of valid
        // memory. whoopsie.

        // XXX(eliza): this read also page faults sometimes. seems wacky.
        let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, 16) };
        tracing::debug!(?bytes);
        // let _ = write!(mk_writer.make_writer(), "  {:016x}: ", ptr).unwrap();
        match decoder.decode_slice(bytes) {
            Ok(inst) => {
                // let _ = writeln!(mk_writer.make_writer(), "{}", inst);
                tracing::error!(target: "oops", "{:016x}: {}", ptr, inst);
                ptr += inst.len();
            }
            Err(e) => {
                // let _ = writeln!(mk_writer.make_writer(), "{}", e);
                tracing::error!(target: "oops", "{:016x}: {}", ptr, e);
                break;
            }
        }
    }
}
