use clap::Parser;
use color_eyre::{eyre::WrapErr, Help};
use inoculate::{Options, Result};

fn main() -> Result<()> {
    color_eyre::install()?;

    let opts = Options::parse();
    opts.output.init()?;

    tracing::info!("inoculating mycelium!");
    tracing::trace!(
        ?opts.cmd,
        ?opts.kernel_bin,
        ?opts.kernel_manifest,
        ?opts.target_dir,
        ?opts.out_dir,
        %opts.bootloader.mode,
        "inoculate configuration"
    );

    let paths = opts.paths()?;

    let image = opts
        .make_image(&paths)
        .context("making the mycelium image didnt work")
        .note("this sucks T_T")?;

    if let Some(cmd) = opts.cmd {
        return cmd.run(image.as_ref(), &paths, opts.bootloader.mode);
    }

    Ok(())
}
