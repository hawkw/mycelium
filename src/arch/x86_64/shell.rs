use crate::shell::{self, Command};

pub const DUMP_ARCH: Command = Command::new("arch")
    .with_help("dump architecture-specific structures")
    .with_subcommands(&[
        Command::new("gdt")
            .with_help("print the global descriptor table (GDT)")
            .with_fn(|_| {
                let gdt = super::interrupt::GDT.get();
                tracing::info!(?gdt);
                Ok(())
            }),
        // Command::new("idt")
        //     .with_help("print the interrupt descriptor table (IDT)")
        //     .with_fn(|_| {
        //         let gdt = super::interrupt::GDT.get();
        //         tracing::info!(?gdt);
        //         Ok(())
        //     }),
    ]);

pub fn dump_timer(_: &str) -> Result<(), shell::Error> {
    tracing::info!(timer = ?super::interrupt::TIMER);
    Ok(())
}
