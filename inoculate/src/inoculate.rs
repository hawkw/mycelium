use crate::{cli, Result};
use clap::Parser;
use color_eyre::{
    eyre::{ensure, format_err, WrapErr},
    Help,
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

mod gdb;
mod qemu;

#[derive(Debug, Parser)]
pub(crate) enum Cmd {
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

impl Cmd {
    pub(crate) fn is_test(&self) -> bool {
        matches!(self, Cmd::Qemu(qemu::Cmd::Test { .. }))
    }

    pub(crate) fn run(&self, image: impl AsRef<Path>, paths: &Paths) -> Result<()> {
        match self {
            Cmd::Qemu(qemu) => qemu.run_qemu(image.as_ref(), paths),
            Cmd::Gdb => gdb::run_gdb(paths.kernel_bin(), 1234).map(|_| ()),
        }
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

    pub fn relative<'path>(&self, path: &'path Path) -> &'path Path {
        path.strip_prefix(self.pwd()).unwrap_or(path)
    }

    pub fn make_image(&self, options: &cli::Options) -> Result<PathBuf> {
        let _span = tracing::info_span!("make_image").entered();

        tracing::info!(
            "Building kernel disk image ({})",
            self.relative(self.kernel_bin()).display()
        );

        tracing::trace!(?self.run_dir);
        let mut cmd = options.cargo_cmd("builder");
        cmd.current_dir(&self.run_dir)
            .arg("--kernel-manifest")
            .arg(&self.kernel_manifest())
            .arg("--kernel-binary")
            .arg(&self.kernel_bin())
            .arg("--out-dir")
            .arg(&self.out_dir)
            .arg("--target-dir")
            .arg(&self.target_dir)
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
        let image = self.out_dir.join(format!("boot-bios-{}.img", bin_name));
        ensure!(
            image.exists(),
            "disk image should probably exist after running bootloader build command"
        );

        tracing::info!(
            "created bootable disk image ({})",
            self.relative(&image).display()
        );

        Ok(image)
    }
}
