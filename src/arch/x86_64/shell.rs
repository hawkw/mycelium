use crate::shell::{Command, NumberFormat};
use mycelium_util::fmt;

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
                "print the value of the specified model-specific register (MSR)\n           \
                MSR_NUM: the MSR number in hexadecimal or binary\n           \
                -f, --fmt <hex|bin|dec>: format the value of the MSR in hexadecimal, \
                decimal, or binary.",
            )
            .with_usage("[-f|--fmt] <MSR_NUM>")
            .with_fn(|mut ctx| {
                let fmt = ctx
                    .parse_optional_flag::<NumberFormat>(&["-f", "--fmt"])?
                    .unwrap_or(NumberFormat::Hex);
                let num = ctx.parse_u32_hex_or_dec("<MSR_NUM>")?;

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
        Command::new("cpuid")
            .with_help(
                "print the value of the specified CPUID leaf (and subleaf)\n           \
                LEAF: the CPUID leaf number in hexadecimal or binary\n           \
                SUBLEAF: the CPUID subleaf number in hexadecimal or binary\n           \
                -f, --fmt <hex|bin|dec>: format the values of the CPUID registers in hexadecimal, \
                decimal, or binary.",
            )
            .with_usage("[-f|--fmt] <LEAF> [SUBLEAF]")
            .with_fn(|mut ctx| {
                use core::arch::x86_64::{CpuidResult, __cpuid_count};
                let fmt = ctx
                    .parse_optional_flag::<NumberFormat>(&["-f", "--fmt"])?
                    .unwrap_or(NumberFormat::Hex);
                let leaf = ctx.parse_u32_hex_or_dec("<LEAF>")?;
                let subleaf = ctx.parse_optional_u32_hex_or_dec("[SUBLEAF]")?.unwrap_or(0);

                let CpuidResult { eax, ebx, ecx, edx } = unsafe { __cpuid_count(leaf, subleaf) };
                match fmt {
                    NumberFormat::Binary => tracing::info!(
                        target: "shell",
                        eax = fmt::bin(eax),
                        ebx = fmt::bin(ebx),
                        ecx = fmt::bin(ecx),
                        edx = fmt::bin(edx),
                        "CPUID {leaf:#x}:{subleaf:x}",
                    ),
                    NumberFormat::Decimal => tracing::info!(
                        target: "shell", eax, ebx, ecx, edx,
                        "CPUID {leaf:#x}:{subleaf:x}",
                    ),
                    NumberFormat::Hex => tracing::info!(
                        target: "shell",
                        eax = fmt::hex(eax),
                        ebx = fmt::hex(ebx),
                        ecx = fmt::hex(ecx),
                        edx = fmt::hex(edx),
                        "CPUID {leaf:#x}:{subleaf:x}",
                    ),
                }
                Ok(())
            }),
    ]);
