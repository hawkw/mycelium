use crate::shell::Command;

pub const DUMP_ARCH: Command = Command::new("arch")
    .with_help("dump architecture-specific structures")
    .with_subcommands(&[
        Command::new("gdt")
            .with_help("print the global descriptor table (GDT)")
            .with_fn(|_| {
                let gdt = super::interrupt::GDT.get();
                tracing::info!(GDT = ?gdt);
                Ok(())
            }),
        Command::new("idt")
            .with_help("print the interrupt descriptor table (IDT)")
            .with_fn(|_| {
                let idt = super::interrupt::Controller::idt();
                tracing::info!(IDT = ?idt);
                Ok(())
            }),
    ]);
