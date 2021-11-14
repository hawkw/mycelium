use color_eyre::{eyre::WrapErr, Help};
use inoculate::{Options, Result};
use structopt::StructOpt;

fn main() -> Result<()> {
    color_eyre::install()?;

    let opts = Options::from_args();
    let color = opts.color;
    color.set_global();
    inoculate::trace::try_init(&opts)?;

    tracing::info!("inoculating mycelium!");
    tracing::debug!(
        ?opts.cmd,
        ?opts.kernel_bin,
        ?opts.bootloader_manifest,
        ?opts.kernel_manifest,
        ?opts.target_dir,
        ?opts.out_dir,
        "inoculate configuration"
    );

    let paths = opts.paths()?;

    let image = opts
        .make_image(&paths)
        .context("making the mycelium image didnt work")
        .note("this sucks T_T")?;

    if let Some(cmd) = opts.cmd {
        return cmd.run(image.as_ref(), &paths);
    }

    Ok(())
}
