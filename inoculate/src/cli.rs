use super::{inoculate::Paths, term};
use color_eyre::{
    eyre::{format_err, Result, WrapErr},
    Help,
};
use std::path::{Path, PathBuf};

#[derive(Debug, clap::Args)]
pub struct Options {
    /// Overrides the path to the `cargo` executable.
    ///
    /// By default, this is read from the `CARGO` environment variable.
    #[clap(
        next_help_heading = "CARGO OPTIONS",
        group = "cargo-opts",
        long = "cargo",
        parse(from_os_str),
        env = "CARGO",
        default_value = "cargo",
        global = true
    )]
    pub(crate) cargo_path: PathBuf,

    #[clap(flatten)]
    pub(crate) output: OutputOptions,

    #[clap(long, env = "GITHUB_ACTIONS", global = true)]
    pub(crate) ci: bool,
}

/// Options that configure the underlying `cargo test` invocation.
#[derive(Debug, clap::Args)]
#[clap(
    next_help_heading = "PATHS",
    group = clap::ArgGroup::new("path-opts")
)]
pub(crate) struct PathOptions {
    /// The path to the `bootloader` crate's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[clap(long, parse(from_os_str), global = true)]
    pub(super) bootloader_manifest: Option<PathBuf>,

    /// The path to the kernel's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[clap(long, parse(from_os_str))]
    pub(super) kernel_manifest: Option<PathBuf>,

    /// Overrides the directory in which to build the output image.
    #[clap(short, long, parse(from_os_str), env = "OUT_DIR", global = true)]
    pub(super) out_dir: Option<PathBuf>,

    /// Overrides the target directory for the kernel build.
    #[clap(
        short,
        long,
        parse(from_os_str),
        env = "CARGO_TARGET_DIR",
        global = true
    )]
    pub(super) target_dir: Option<PathBuf>,
}

/// Options that configure `inoculate`'s output.
#[derive(Debug, clap::Args)]
#[clap(
    next_help_heading = "OUTPUT OPTIONS",
    group = clap::ArgGroup::new("output-opts")
)]
pub(crate) struct OutputOptions {
    /// Configures build logging.
    #[clap(
        short,
        long,
        env = "RUST_LOG",
        default_value = "inoculate=info,warn",
        global = true
    )]
    log: tracing_subscriber::EnvFilter,

    /// Whether to emit colors in output.
    #[clap(
        long,
        possible_values(&["auto", "always", "never"]),
        env = "CARGO_TERM_COLORS",
        default_value = "auto",
        global = true
    )]
    pub(crate) color: term::ColorMode,
}

/// Options that configure `cargo` invocations.
#[derive(Debug, clap::Args)]
#[clap(
    next_help_heading = "CARGO OPTIONS",
    group = clap::ArgGroup::new("cargo-opts")
)]
pub(crate) struct CargoOptions {
    #[clap(flatten)]
    pub(crate) features: clap_cargo::Features,

    #[clap(flatten)]
    pub(crate) workspace: clap_cargo::Workspace,

    #[clap(flatten)]
    pub(crate) manifest: clap_cargo::Manifest,
}

// === impl Options ===

impl Options {
    pub fn init_term(&mut self) -> Result<()> {
        self.output.init_term()
    }
}

// === impl PathOptions ===

impl PathOptions {
    fn wheres_the_kernel_bin(&self, kernel_bin: &Path) -> Result<PathBuf> {
        tracing::debug!("where's the kernel binary?");
        kernel_bin
            // the bootloader crate's build script gets mad if this is a
            // relative path
            .canonicalize()
            .context("couldn't to canonicalize kernel manifest path")
            .note("it should work")
    }

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

    pub(super) fn paths(&self, kernel_bin: impl AsRef<Path>) -> Result<Paths> {
        let bootloader_manifest = self.wheres_bootloader()?;
        tracing::info!(path = %bootloader_manifest.display(), "found bootloader manifest");

        let kernel_manifest = self.wheres_the_kernel()?;
        tracing::info!(path = %kernel_manifest.display(), "found kernel manifest");

        let kernel_bin = self.wheres_the_kernel_bin(kernel_bin.as_ref())?;
        tracing::info!(path = %kernel_bin.display(), "found kernel binary");

        let pwd = std::env::current_dir().unwrap_or_else(|error| {
            tracing::warn!(?error, "error getting current dir");
            Default::default()
        });
        tracing::debug!(path = %pwd.display(), "found pwd");

        let out_dir = self
            .out_dir
            .as_ref()
            .map(|path| path.as_ref())
            .or_else(|| kernel_bin.parent())
            .ok_or_else(|| format_err!("can't find out dir, wtf"))
            .context("determining out dir")
            .note("somethings messed up lol")?
            .to_path_buf();

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
            .suggestion("maybe dont run this in `/`???")?
            .to_path_buf();

        Ok(Paths {
            bootloader_manifest,
            kernel_manifest,
            kernel_bin,
            pwd,
            out_dir,
            target_dir,
            run_dir,
        })
    }
}

// === impl OutputOptions ===

impl OutputOptions {
    pub(crate) fn init_term(&mut self) -> Result<()> {
        use crate::trace;
        use tracing_subscriber::prelude::*;

        term::init_color_mode(self.color)?;

        let fmt = tracing_subscriber::fmt::layer()
            .event_format(trace::CargoFormatter::new(self.color))
            .with_writer(std::io::stderr);

        tracing_subscriber::registry()
            .with(fmt)
            .with(tracing_error::ErrorLayer::default())
            .with(std::mem::take(&mut self.log))
            .try_init()?;
        Ok(())
    }
}
