use clap::Parser;
use color_eyre::{
    eyre::{ensure, format_err, WrapErr},
    Help,
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

pub use color_eyre::eyre::Result;
pub mod cargo;
pub mod gdb;
pub mod qemu;
pub mod term;
pub mod trace;

#[derive(Debug, Parser)]
#[clap(about, version, author = "Eliza Weisman <eliza@elizas.website>")]
pub struct Options {
    /// Which command to run?
    ///
    /// By default, an image is built but not run.
    #[clap(subcommand)]
    pub cmd: Option<Subcommand>,

    /// Configures build logging.
    #[clap(short, long, env = "RUST_LOG", default_value = "inoculate=info,warn")]
    pub log: String,

    /// The path to the kernel binary.
    ///
    /// When inoculate is used as a Cargo runner (which it typically is), this
    /// is passed by Cargo.
    #[clap(parse(from_os_str))]
    pub kernel_bin: PathBuf,

    /// Overrides the path to the `bootloader` crate's Cargo manifest.
    ///
    /// If this is not provided, it will be located automatically.
    #[clap(long, parse(from_os_str))]
    pub bootloader_manifest: Option<PathBuf>,

    /// Overrides the path to the kernel's Cargo manifest.
    ///
    /// If this is not provided, it will be located automatically.
    #[clap(long, parse(from_os_str))]
    pub kernel_manifest: Option<PathBuf>,

    /// Overrides the directory in which to build the output image.
    #[clap(short, long, parse(from_os_str), env = "OUT_DIR")]
    pub out_dir: Option<PathBuf>,

    /// Overrides the target directory for the kernel build.
    #[clap(short, long, parse(from_os_str), env = "CARGO_TARGET_DIR")]
    pub target_dir: Option<PathBuf>,

    /// Overrides the path to the `cargo` executable.
    ///
    /// By default, this is read from the `CARGO` environment variable.
    #[clap(
        long = "cargo",
        parse(from_os_str),
        env = "CARGO",
        default_value = "cargo"
    )]
    pub cargo_path: PathBuf,

    /// Whether to emit colors in output.
    #[clap(
        long,
        possible_values(&["auto", "always", "never"]),
        env = "CARGO_TERM_COLORS",
        default_value = "auto"
    )]
    pub color: term::ColorMode,
}

#[derive(Debug, Parser)]
pub enum Subcommand {
    #[clap(flatten)]
    Qemu(qemu::Cmd),
    /// Run `gdb` without launching the kernel in QEMU.
    ///
    /// This assumes QEMU was already started by a separate `cargo inoculate`
    /// invocation, and that invocation was configured to listen for a GDB
    /// connection on the default port.
    Gdb,
}

#[derive(Debug)]
pub struct Paths {
    pub pwd: PathBuf,
    pub kernel_bin: PathBuf,
    pub kernel_manifest: PathBuf,
    pub bootloader_manifest: PathBuf,
}

impl Subcommand {
    pub fn run(&self, image: &Path, paths: &Paths) -> Result<()> {
        match self {
            Subcommand::Qemu(qemu) => qemu.run_qemu(image, paths),
            Subcommand::Gdb => crate::gdb::run_gdb(paths.kernel_bin(), 1234).map(|_| ()),
        }
    }
}

impl Options {
    pub fn trace_init(&self) -> Result<()> {
        trace::try_init(self)
    }

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

    pub fn paths(&self) -> Result<Paths> {
        let bootloader_manifest = self.wheres_bootloader()?;
        tracing::info!(path = %bootloader_manifest.display(), "found bootloader manifest");

        let kernel_manifest = self.wheres_the_kernel()?;
        tracing::info!(path = %kernel_manifest.display(), "found kernel manifest");

        let kernel_bin = self.wheres_the_kernel_bin()?;
        tracing::info!(path = %kernel_bin.display(), "found kernel binary");

        let pwd = std::env::current_dir().unwrap_or_else(|error| {
            tracing::warn!(?error, "error getting current dir");
            Default::default()
        });
        tracing::debug!(path = %pwd.display(), "found pwd");

        Ok(Paths {
            bootloader_manifest,
            kernel_manifest,
            kernel_bin,
            pwd,
        })
    }

    pub fn make_image(&self, paths: &Paths) -> Result<PathBuf> {
        let _span = tracing::info_span!("make_image").entered();

        tracing::info!(
            "Building kernel disk image ({})",
            paths.relative(paths.kernel_bin()).display()
        );

        let out_dir = self
            .out_dir
            .as_ref()
            .map(|path| path.as_ref())
            .or_else(|| paths.kernel_bin().parent())
            .ok_or_else(|| format_err!("can't find out dir, wtf"))
            .context("determining out dir")
            .note("somethings messed up lol")?;
        let target_dir = self
            .target_dir
            .clone()
            .or_else(|| Some(paths.kernel_manifest().parent()?.join("target")))
            .ok_or_else(|| format_err!("can't find target dir, wtf"))
            .context("determining target dir")
            .note("somethings messed up lol")?;
        let run_dir = paths
            .bootloader_manifest()
            .parent()
            .ok_or_else(|| format_err!("bootloader manifest path doesn't have a parent dir"))
            .note("thats messed up lol")
            .suggestion("maybe dont run this in `/`???")?;

        tracing::trace!(?run_dir);
        let mut cmd = self.cargo_cmd("builder");
        cmd.current_dir(run_dir)
            .arg("--kernel-manifest")
            .arg(&paths.kernel_manifest())
            .arg("--kernel-binary")
            .arg(&paths.kernel_bin())
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--target-dir")
            .arg(target_dir)
            .stderr(Stdio::inherit())
            .stdout(Stdio::piped());
        tracing::debug!(?cmd, "running bootimage builder");
        let output = cmd.status().context("run builder command")?;
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

        tracing::info!(
            "created bootable disk image ({})",
            paths.relative(&image).display()
        );

        Ok(image)
    }
}

// === impl Paths ===

impl Paths {
    pub fn kernel_bin(&self) -> &Path {
        self.kernel_bin.as_ref()
    }

    pub fn kernel_manifest(&self) -> &Path {
        self.kernel_manifest.as_ref()
    }

    pub fn bootloader_manifest(&self) -> &Path {
        self.bootloader_manifest.as_ref()
    }

    pub fn pwd(&self) -> &Path {
        self.pwd.as_ref()
    }

    pub fn relative<'path>(&self, path: &'path Path) -> &'path Path {
        path.strip_prefix(self.pwd()).unwrap_or(path)
    }
}
