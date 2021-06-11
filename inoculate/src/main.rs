use color_eyre::{eyre::WrapErr, Help};
use inoculate::{Options, Result};
use structopt::StructOpt;

fn main() -> Result<()> {
    use tracing_subscriber::prelude::*;
    color_eyre::install()?;

    let opts = Options::from_args();
    let color = opts.color;
    color.set_global();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_ansi(color.should_color_stdout()))
        .with(tracing_error::ErrorLayer::default())
        .with(opts.log.parse::<tracing_subscriber::EnvFilter>()?)
        .init();

    tracing::info! {
        ?opts.qemu,
        ?opts.kernel_bin,
        ?opts.bootloader_manifest,
        ?opts.kernel_manifest,
        ?opts.target_dir,
        ?opts.out_dir,
        "inoculating...",
    };

    // if opts.is_test() {
    //     let rustflags = if let Ok(mut rustflags) = std::env::var("RUSTFLAGS") {
    //         rustflags.push_str(" --cfg test");
    //         rustflags
    //     } else {
    //         String::from("--cfg test")
    //     };
    //     tracing::info!(opts.is_test = true, ?rustflags);
    //     std::env::set_var("RUSTFLAGS", rustflags);
    // }

    let bootloader_manifest = opts.wheres_bootloader()?;
    tracing::info!(path = %bootloader_manifest.display(), "found bootloader manifest");

    let kernel_manifest = opts.wheres_the_kernel()?;
    tracing::info!(path = %kernel_manifest.display(), "found kernel manifest");

    let image = opts
        .make_image(bootloader_manifest.as_ref(), kernel_manifest.as_ref())
        .context("making the mycelium image didnt work")
        .note("this sucks T_T")?;
    tracing::info!(image = %image.display());

    if let Some(qemu) = opts.qemu {
        return qemu.run_qemu(image.as_ref());
    }

    Ok(())
}
