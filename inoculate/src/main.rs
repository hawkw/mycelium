use clap::Parser;
use color_eyre::{eyre::WrapErr, Help};
use inoculate::{Options, Result};

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut opts = Options::parse();
    opts.trace_init()?;

    tracing::info!("inoculating mycelium!");
    let paths = opts.paths()?;
    tracing::trace!(
        ?opts.cmd,
        ?paths.kernel_bin,
        ?paths.bootloader_manifest,
        ?paths.kernel_manifest,
        ?paths.target_dir,
        ?paths.out_dir,
        "inoculate configuration"
    );

    let image = opts
        .make_image(&paths)
        .context("making the mycelium image didnt work")
        .note("this sucks T_T")?;

    if let Some(cmd) = opts.cmd {
        return cmd.run(image.as_ref(), &paths);
    }

    Ok(())
}
