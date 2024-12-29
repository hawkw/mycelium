use crate::shell::{Command, NumberFormat};

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
        Command::new("msr")
            .with_help(
                "print the value of the specified model-specific register (MSR)\n\
                MSR_NUM is a hexadecimal or decimal number\n\
                -f, --fmt <hex|bin|dec>: format the value of the MSR in hexadecimal, decimal, or binary."
            )
            .with_usage("[-f|--fmt] <MSR_NUM>")
            .with_fn(|mut ctx| {
                let fmt = ctx.parse_optional_flag::<NumberFormat>(&["-f", "--fmt"]).unwrap_or(Ok(NumberFormat::Hex))?;
                let num = ctx.parse_u32_hex_or_dec()?;

                let msr = hal_x86_64::cpu::msr::Msr::try_new(num).ok_or_else(|| {
                    ctx.other_error(
                        "CPU does not support model-specific registers (must be pre-pentium...)",
                    )
                })?;

                let val = msr.read_raw();
                match fmt {
                    NumberFormat::Binary => tracing::info!("MSR {num:#x} = {val:#b}"),
                    NumberFormat::Decimal => tracing::info!("MSR {num:#x} = {val}"),
                    NumberFormat::Hex => tracing::info!("MSR {num:#x} = {val:#x}"),
                }
                Ok(())
            }),
    ]);
