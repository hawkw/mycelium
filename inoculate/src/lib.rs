use color_eyre::{
    eyre::{ensure, format_err, WrapErr},
    Help,
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use structopt::StructOpt;

pub use color_eyre::eyre::Result;

pub mod cargo;
pub mod gdb;
pub mod qemu;
pub mod term;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "inoculate",
    about = "the horrible mycelium build tool (because that's a thing we have to have now apparently!)"
)]
pub struct Options {
    /// Which command to run?
    ///
    /// By default, an image is built but not run.
    #[structopt(subcommand)]
    pub cmd: Option<Subcommand>,

    /// Configures build logging.
    #[structopt(short, long, env = "RUST_LOG", default_value = "warn")]
    pub log: String,

    /// The path to the kernel binary.
    #[structopt(parse(from_os_str))]
    pub kernel_bin: PathBuf,

    /// The path to the `bootloader` crate's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[structopt(long, parse(from_os_str))]
    pub bootloader_manifest: Option<PathBuf>,

    /// The path to the kernel's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[structopt(long, parse(from_os_str))]
    pub kernel_manifest: Option<PathBuf>,

    /// Overrides the directory in which to build the output image.
    #[structopt(short, long, parse(from_os_str))]
    pub out_dir: Option<PathBuf>,

    /// Overrides the target directory for the kernel build.
    #[structopt(short, long, parse(from_os_str))]
    pub target_dir: Option<PathBuf>,

    /// Whether to emit colors in output.
    #[structopt(
        long,
        possible_values(&["auto", "always", "never"]),
        env = "CARGO_TERM_COLORS",
        default_value = "auto"
    )]
    pub color: term::ColorMode,
}

#[derive(Debug, StructOpt)]
pub enum Subcommand {
    #[structopt(flatten)]
    Qemu(qemu::Cmd),
    /// Run `gdb` without launching the kernel in QEMU.
    ///
    /// This assumes QEMU was already started by a separate `cargo inoculate`
    /// invocation, and that invocation was configured to listen for a GDB
    /// connection on the default port.
    Gdb,
}

impl Subcommand {
    pub fn run(&self, image: &Path, kernel_bin: &Path) -> Result<()> {
        match self {
            Subcommand::Qemu(qemu) => qemu.run_qemu(image, kernel_bin),
            Subcommand::Gdb => crate::gdb::run_gdb(kernel_bin, 1234).map(|_| ()),
        }
    }
}

impl Options {
    pub fn is_test(&self) -> bool {
        matches!(self.cmd, Some(Subcommand::Qemu(qemu::Cmd::Test { .. })))
    }

    pub fn wheres_bootloader(&self) -> Result<PathBuf> {
        tracing::debug!("where's bootloader?");
        if let Some(path) = self.bootloader_manifest.as_ref() {
            tracing::info!(path = %path.display(), "bootloader path overridden");
            return Ok(path.clone());
        }
        bootloader_locator::locate_bootloader("bootloader")
            .note("where the hell is the `bootloader` crate's Cargo.toml?")
            .suggestion("maybe you forgot to depend on it")
    }

    pub fn wheres_the_kernel(&self) -> Result<PathBuf> {
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

    pub fn wheres_the_kernel_bin(&self) -> Result<PathBuf> {
        tracing::debug!("where's the kernel binary?");
        self.kernel_bin
            // the bootloader crate's build script gets mad if this is a
            // relative path
            .canonicalize()
            .context("couldn't to canonicalize kernel manifest path")
            .note("it should work")
    }

    pub fn make_image(
        &self,
        bootloader_manifest: &Path,
        kernel_manifest: &Path,
        kernel_bin: &Path,
    ) -> Result<PathBuf> {
        let _span = tracing::info_span!("make_image").entered();

        cargo_log!("Building", "disk image for `{}``s", kernel_bin.display());

        tracing::debug!(kernel_bin = %kernel_bin.display(), "making boot image...");
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
        let output = cargo::cmd("builder")?
            .current_dir(run_dir)
            .arg("--kernel-manifest")
            .arg(&kernel_manifest)
            .arg("--kernel-binary")
            .arg(&kernel_bin)
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--target-dir")
            .arg(target_dir)
            .stderr(Stdio::inherit())
            .stdout(Stdio::piped())
            .status()
            .context("run builder command")?;
        // TODO(eliza): modes for capturing/piping stdout?

        if !output.success() {
            return Err(format_err!(
                "bootloader's builder command exited with non-zero status code"
            ))
            .suggestion("if you had gotten all the inputs right, this should have worked");
        }

        let bin_name = self.kernel_bin.file_name().unwrap().to_str().unwrap();
        let image = out_dir.join(format!("boot-bios-{}.img", bin_name));
        ensure!(
            image.exists(),
            "disk image should probably exist after running bootloader build command"
        );

        cargo_log!("Created", "bootable disk image at `{}`", image.display());

        Ok(image)
    }
}
