use clap::{ArgGroup, Args, Parser, ValueHint};
use color_eyre::{
    eyre::{format_err, WrapErr},
    Help,
};
use std::{
    fmt,
    path::{Path, PathBuf},
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

    /// The path to the kernel binary.
    #[clap(value_hint = ValueHint::FilePath)]
    pub kernel_bin: PathBuf,

    /// The path to the kernel's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[clap(value_hint = ValueHint::FilePath)]
    #[clap(long, global = true)]
    pub kernel_manifest: Option<PathBuf>,

    /// Overrides the directory in which to build the output image.
    #[clap(short, long, env = "OUT_DIR", value_hint = ValueHint::DirPath, global = true)]
    pub out_dir: Option<PathBuf>,

    /// Overrides the target directory for the kernel build.
    #[clap(
        short,
        long,
        env = "CARGO_TARGET_DIR",
        value_hint = ValueHint::DirPath,
        global = true
    )]
    pub target_dir: Option<PathBuf>,

    /// Overrides the path to the `cargo` executable.
    ///
    /// By default, this is read from the `CARGO` environment variable.
    #[clap(
        long = "cargo",
        env = "CARGO",
        default_value = "cargo",
        value_hint = ValueHint::ExecutablePath,
        global = true
    )]
    pub cargo_path: PathBuf,

    #[clap(flatten)]
    pub output: term::OutputOptions,

    /// Configures the bootloader.
    #[clap(flatten)]
    pub bootloader: BootloaderOptions,
}

#[derive(Clone, Debug, Args)]
#[command(
    next_help_heading = "Bootloader Options",
    group = ArgGroup::new(Self::ARG_GROUP).multiple(true),
)]
pub struct BootloaderOptions {
    /// How to boot Mycelium.
    ///
    /// This determines which type of image is built, and (if a QEMU subcommand
    /// is executed) how QEMU will boot Mycelium.
    #[clap(
        long = "boot",
        short = 'b',
        default_value_t = BootMode::Uefi,
        global = true,
        group = Self::ARG_GROUP,
    )]
    pub mode: BootMode,

    /// Log level for the bootloader.
    #[clap(
        long,
        default_value_t = BootLogLevel::Info,
        global = true,
        group = Self::ARG_GROUP,
    )]
    boot_log: BootLogLevel,

    /// Instructs the bootloader to set up a framebuffer format that has at least the given height.
    ///
    /// If this is not possible, the bootloader will fall back to a smaller format.
    #[clap(long, global = true, group = Self::ARG_GROUP)]
    framebuffer_height: Option<u64>,

    /// Instructs the bootloader to set up a framebuffer format that has at least the given width.
    ///
    /// If this is not possible, the bootloader will fall back to a smaller format.
    #[clap(long, global = true, group = Self::ARG_GROUP)]
    framebuffer_width: Option<u64>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
#[repr(u8)]
#[clap(rename_all = "upper")]
pub enum BootMode {
    /// Boot mycelium using the UEFI bootloader.
    ///
    /// The kernel image will be output to `<OUT_DIR>/uefi.img`.
    Uefi,
    /// Boot mycelium using the legacy BIOS bootloader.
    ///
    /// The kernel image will be output to `<OUT_DIR>/bios.img`.
    Bios,
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
    pub out_dir: PathBuf,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
#[repr(u8)]
enum BootLogLevel {
    /// A level lower than all log levels.
    Off,
    /// Corresponds to the `Error` log level.
    Error,
    /// Corresponds to the `Warn` log level.
    Warn,
    /// Corresponds to the `Info` log level.
    Info,
    /// Corresponds to the `Debug` log level.
    Debug,
    /// Corresponds to the `Trace` log level.
    Trace,
}

impl Subcommand {
    pub fn run(&self, image: &Path, paths: &Paths, boot: BootMode) -> Result<()> {
        match self {
            Subcommand::Qemu(qemu) => qemu.run_qemu(image, paths, boot),
            Subcommand::Gdb => crate::gdb::run_gdb(paths.kernel_bin(), 1234).map(|_| ()),
        }
    }
}

impl Options {
    pub fn is_test(&self) -> bool {
        matches!(self.cmd, Some(Subcommand::Qemu(qemu::Cmd::Test { .. })))
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
        let kernel_manifest = self.wheres_the_kernel()?;
        tracing::info!(path = %kernel_manifest.display(), "found kernel manifest");

        let kernel_bin = self.wheres_the_kernel_bin()?;
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
        tracing::debug!(path = %out_dir.display(), "determined output directory");

        Ok(Paths {
            kernel_manifest,
            kernel_bin,
            pwd,
            out_dir,
        })
    }

    pub fn make_image(&self, paths: &Paths) -> Result<PathBuf> {
        let _span = tracing::info_span!("make_image").entered();

        tracing::info!(
            img = %paths.relative(paths.kernel_bin()).display(),
            boot = %self.bootloader.mode,
            "Building kernel disk image",
        );

        let bootcfg = self.bootloader.boot_config();
        let path = match self.bootloader.mode {
            BootMode::Uefi => {
                let path = paths.uefi_img();
                let mut builder = bootloader::UefiBoot::new(paths.kernel_bin());
                builder.set_boot_config(&bootcfg);
                builder
                    .create_disk_image(&path)
                    .map_err(|error| format_err!("failed to build UEFI image: {error}"))
                    .with_note(|| format!("output path: {}", path.display()))?;
                path
            }
            BootMode::Bios => {
                let path = paths.bios_img();
                let mut builder = bootloader::BiosBoot::new(paths.kernel_bin());
                builder.set_boot_config(&bootcfg);
                builder
                    .create_disk_image(&path)
                    .map_err(|error| format_err!("failed to build BIOS image: {error}"))
                    .with_note(|| format!("output path: {}", path.display()))?;
                path
            }
        };

        tracing::info!(
            "Created bootable disk image ({})",
            paths.relative(&path).display()
        );

        Ok(path)
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

    pub fn pwd(&self) -> &Path {
        self.pwd.as_ref()
    }

    pub fn uefi_img(&self) -> PathBuf {
        self.out_dir.join("uefi.img")
    }

    pub fn bios_img(&self) -> PathBuf {
        self.out_dir.join("bios.img")
    }

    pub fn relative<'path>(&self, path: &'path Path) -> &'path Path {
        path.strip_prefix(self.pwd()).unwrap_or(path)
    }
}

// === impl BootloaderOptions ===

impl BootloaderOptions {
    const ARG_GROUP: &'static str = "boot-opts";

    fn boot_config(&self) -> bootloader_boot_config::BootConfig {
        let mut bootcfg = bootloader::BootConfig::default();
        bootcfg.log_level = self.boot_log.into();
        bootcfg.frame_buffer_logging = true;
        bootcfg.serial_logging = true;
        if self.framebuffer_height.is_some() {
            bootcfg.frame_buffer.minimum_framebuffer_height = self.framebuffer_height;
        }
        if self.framebuffer_width.is_some() {
            bootcfg.frame_buffer.minimum_framebuffer_width = self.framebuffer_width;
        }
        tracing::debug!(
            ?bootcfg.log_level,
            bootcfg.frame_buffer_logging,
            bootcfg.serial_logging,
            ?bootcfg.frame_buffer
        );
        bootcfg
    }
}

// === impl BootMode ===

impl fmt::Display for BootMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BootMode::Uefi => f.pad("UEFI"),
            BootMode::Bios => f.pad("BIOS"),
        }
    }
}

// === impl BootLogLevel ===

impl fmt::Display for BootLogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
        .fmt(f)
    }
}

impl From<BootLogLevel> for bootloader_boot_config::LevelFilter {
    fn from(level: BootLogLevel) -> bootloader_boot_config::LevelFilter {
        use bootloader_boot_config::LevelFilter;
        match level {
            BootLogLevel::Off => LevelFilter::Off,
            BootLogLevel::Error => LevelFilter::Error,
            BootLogLevel::Warn => LevelFilter::Warn,
            BootLogLevel::Info => LevelFilter::Info,
            BootLogLevel::Debug => LevelFilter::Debug,
            BootLogLevel::Trace => LevelFilter::Trace,
        }
    }
}
