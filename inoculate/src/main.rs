use color_eyre::{
    eyre::{ensure, format_err, Result, WrapErr},
    Help, SectionExt,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "inoculate",
    about = "the horrible mycelium build tool (because that's a thing we have to have now apparently!)"
)]
struct Options {
    /// Which command to run?
    ///
    /// By default, an image is built but not run.
    #[structopt(subcommand)]
    cmd: Option<Subcommand>,

    /// Configures build logging.
    #[structopt(short, long, env = "RUST_LOG", default_value = "warn")]
    log: String,

    /// The path to the kernel binary.
    #[structopt(parse(from_os_str))]
    kernel_bin: PathBuf,

    /// The path to the `bootloader` crate's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[structopt(long, parse(from_os_str))]
    bootloader_manifest: Option<PathBuf>,

    /// The path to the kernel's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[structopt(long, parse(from_os_str))]
    kernel_manifest: Option<PathBuf>,

    /// Overrides the directory in which to build the output image.
    #[structopt(short, long, parse(from_os_str))]
    out_dir: Option<PathBuf>,

    /// Overrides the target directory for the kernel build.
    #[structopt(short, long, parse(from_os_str))]
    target_dir: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
enum Subcommand {
    /// Builds a bootable disk image and runs it in QEMU (implies: `build`).
    Run,
    /// Builds a bootable disk image with tests enabled, and runs the tests in QEMU.
    Test,
}

impl Options {
    fn wheres_bootloader(&self) -> Result<PathBuf> {
        tracing::debug!("where's bootloader?");
        if let Some(path) = self.bootloader_manifest.as_ref() {
            tracing::info!(path = %path.display(), "bootloader path overridden");
            return Ok(path.clone());
        }
        bootloader_locator::locate_bootloader("bootloader")
            .note("where the hell is the `bootloader` crate's Cargo.toml?")
            .suggestion("maybe you forgot to depend on it")
    }

    fn wheres_the_kernel(&self) -> Result<PathBuf> {
        tracing::debug!("where's the kernel?");
        if let Some(path) = self.kernel_manifest.as_ref() {
            tracing::info!(path = %path.display(), "kernel manifest path path overridden");
            return Ok(path.clone());
        }
        locate_cargo_manifest::locate_manifest()
            .note("where the hell is the kernel's Cargo.toml?")
            .note("this should never happen, seriously, wtf")
            .suggestion("have you tried not having it be missing")
    }

    #[tracing::instrument]
    fn make_image(&self, bootloader_manifest: &Path, kernel_manifest: &Path) -> Result<PathBuf> {
        let kernel_bin = self
            .kernel_bin
            // the bootloader crate's build script gets mad if this is a
            // relative pathe
            .canonicalize()
            .context("couldn't to canonicalize kernel manifest path")
            .note("it should work")?;
        let out_dir = self
            .out_dir
            .as_ref()
            .map(|path| path.as_ref())
            .or_else(|| kernel_bin.parent())
            .ok_or_else(|| format_err!("can't find out dir, wtf"))
            .context("determining out dir")
            .note("somethings messed up lol")?;
        let target_dir = self
            .target_dir
            .clone()
            .or_else(|| Some(kernel_manifest.parent()?.join("target")))
            .ok_or_else(|| format_err!("can't find target dir, wtf"))
            .context("determining target dir")
            .note("somethings messed up lol")?;
        let run_dir = bootloader_manifest
            .parent()
            .ok_or_else(|| format_err!("bootloader manifest path doesn't have a parent dir"))
            .note("thats messed up lol")
            .suggestion("maybe dont run this in `/`???")?;
        let output = Command::new(env!("CARGO"))
            .current_dir(run_dir)
            .arg("builder")
            .arg("--kernel-manifest")
            .arg(&kernel_manifest)
            .arg("--kernel-binary")
            .arg(&kernel_bin)
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--target-dir")
            .arg(target_dir)
            .status()
            .context("run builder command")?;
        // TODO(eliza): modes for capturing/piping stdout?

        // let stdout = String::from_utf8_lossy(&output.stdout);
        // if !output.status.success() {
        if !output.success() {
            // let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format_err!(
                "bootloader's builder command exited with non-zero status code"
            ))
            // .with_section(move || stdout.trim().to_string().header("stdout:"))
            // .with_section(move || stderr.trim().to_string().header("stderr:"))
            .suggestion("if you had gotten all the inputs right, this should have worked");
        }

        let bin_name = self.kernel_bin.file_name().unwrap().to_str().unwrap();
        let image = out_dir.join(format!("boot-bios-{}.img", bin_name));
        ensure!(
            image.exists(),
            "disk image should probably exist after running bootloader build command"
        );

        Ok(image)
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let opts = Options::from_args();
    tracing_subscriber::fmt()
        .with_env_filter(opts.log.parse::<tracing_subscriber::EnvFilter>()?)
        .init();

    let bootloader_manifest = opts.wheres_bootloader()?;
    tracing::info!(path = %bootloader_manifest.display(), "found bootloader manifest");

    let kernel_manifest = opts.wheres_the_kernel()?;
    tracing::info!(path = %kernel_manifest.display(), "found kernel manifest");

    let image = opts
        .make_image(bootloader_manifest.as_ref(), kernel_manifest.as_ref())
        .context("making the mycelium image didnt work")
        .note("this sucks T_T")?;
    tracing::info!(image = %image.display());

    Ok(())
}
