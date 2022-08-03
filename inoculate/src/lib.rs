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
pub mod cli;
pub mod gdb;
pub mod qemu;
pub mod term;
pub mod trace;

#[derive(Debug, Parser)]
#[clap(about, version, author = "Eliza Weisman <eliza@elizas.website>")]
pub struct Options {
    /// The path to the kernel binary.
    #[clap(parse(from_os_str))]
    kernel_bin: PathBuf,

    /// Which command to run?
    ///
    /// By default, an image is built but not run.
    #[clap(subcommand)]
    pub cmd: Option<Subcommand>,

    #[clap(flatten)]
    paths: cli::PathOptions,

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

    #[clap(flatten)]
    pub(crate) output: cli::OutputOptions,
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
    pub out_dir: PathBuf,
    pub target_dir: PathBuf,
    pub run_dir: PathBuf,
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
    pub fn trace_init(&mut self) -> Result<()> {
        self.output.trace_init()
    }

    pub fn is_test(&self) -> bool {
        matches!(self.cmd, Some(Subcommand::Qemu(qemu::Cmd::Test { .. })))
    }

    pub fn make_image(&self, paths: &Paths) -> Result<PathBuf> {
        let _span = tracing::info_span!("make_image").entered();

        tracing::info!(
            "Building kernel disk image ({})",
            paths.relative(paths.kernel_bin()).display()
        );

        tracing::trace!(?paths.run_dir);
        let mut cmd = self.cargo_cmd("builder");
        cmd.current_dir(&paths.run_dir)
            .arg("--kernel-manifest")
            .arg(&paths.kernel_manifest())
            .arg("--kernel-binary")
            .arg(&paths.kernel_bin())
            .arg("--out-dir")
            .arg(&paths.out_dir)
            .arg("--target-dir")
            .arg(&paths.target_dir)
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

        let bin_name = paths.kernel_bin.file_name().unwrap().to_str().unwrap();
        let image = paths.out_dir.join(format!("boot-bios-{}.img", bin_name));
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

    pub fn paths(&self) -> Result<Paths> {
        self.paths.paths(&self.kernel_bin)
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
