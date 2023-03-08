use crate::shell::Command;

pub const DUMP_ARCH: Command = Command::new("arch")
    .with_help("dump architecture-specific structures")
    .with_subcommands(&[
        Command::new("gdt")
            .with_help("print the global descriptor table (GDT)")
            .with_fn(|_| {
                let gdt = super::segmentation::GDT.lock();
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
        Command::new("cpu")
            .with_help("print information about a CPU (or the entire CPU topology)")
            .with_usage("[CPU_NUM]")
            .with_fn(|ctx| {
                let topology = super::TOPOLOGY.lock();
                let Some(ref topology) = &*topology else {
                    tracing::warn!(
                        "CPU topology not detected (does the system have ACPI tables?)"
                    );
                    return Ok(());
                };

                let line = ctx.command().trim();

                // no CPU number, dump the whole topology
                if line.is_empty() {
                    tracing::info!(?topology);
                    return Ok(());
                }

                let cpu_num: usize = line
                    .parse()
                    .map_err(|_| ctx.invalid_argument("CPU number must be an integer"))?;

                if cpu_num == 0 {
                    tracing::info!(?topology.boot_processor);
                    return Ok(());
                }

                match topology.application_processors.get(cpu_num - 1) {
                    Some(cpu) => tracing::info!(cpu_num, ?cpu),
                    None => tracing::warn!("CPU {} not found", cpu_num),
                }

                Ok(())
            }),
    ]);
