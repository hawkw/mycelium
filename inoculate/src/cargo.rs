use crate::cli;
use cargo_metadata::MetadataCommand;
use color_eyre::eyre::{self, Result, WrapErr};
use std::process::Command;

impl cli::Options {
    pub fn cargo_cmd(&self, cmd: &str) -> Command {
        let mut cargo = Command::new(self.cargo_path.as_os_str());
        cargo
            .arg(cmd)
            // propagate our color mode configuration
            .env("CARGO_TERM_COLOR", self.output.color.as_str());
        if let Some(ref manifest_path) = self.manifest.manifest_path {
            cargo.arg("--manifest-path").arg(manifest_path);
        }
        cargo
    }

    pub fn metadata_command(&self) -> MetadataCommand {
        let mut cmd = self.manifest.metadata();
        cmd.cargo_path(&self.cargo_path);
        cmd
    }

    pub fn install_nextest(&self) -> Result<bool> {
        self.install_subcommand("nextest")
            .context("failed to find `cargo nextest`")
    }

    pub fn install_subcommand(&self, subcommand: &str) -> Result<bool> {
        if self.has_subcommand(subcommand)? {
            return Ok(true);
        }

        if self.confirm(format_args!("missing `cargo {subcommand}`, install it?")) {
            let mut install = self.cargo_cmd("install");
            install.arg(format!("cargo-{subcommand}")).arg("-f");
            tracing::debug!(cmd = ?install, installing = %subcommand, "running");
            let status = install
                .status()
                .with_context(|| format!("running {install:?}"))?;
            if !status.success() {
                eyre::bail!("running {install:?} failed, status: {status:?}");
            }

            return Ok(true);
        }

        Ok(false)
    }

    pub fn has_subcommand(&self, command: &str) -> Result<bool> {
        (|| {
            let output = self
                .cargo_cmd("--list")
                .output()
                .context("running `cargo --list` failed")?;

            if !output.status.success() {
                eyre::bail!("`cargo --list` failed, status: {:?}", output.status);
            }

            let stdout = std::str::from_utf8(&output.stdout)
                .context("parsing `cargo --list` output failed")?;
            Ok::<bool, eyre::Error>(stdout.contains(command))
        })()
        .with_context(|| format!("checking for `cargo {command}` failed"))
    }
}

impl cli::CargoOptions {
    pub fn configure_with_crates<'cmd>(
        &'cmd self,
        defaults: &[&'cmd str],
        cmd: &'cmd mut Command,
    ) -> &'cmd mut Command {
        let cmd = self.configure_cmd(cmd);
        self.crates_or_defaults(defaults, cmd)
    }
    pub fn configure_cmd<'cmd>(&'cmd self, cmd: &'cmd mut Command) -> &'cmd mut Command {
        if self.features.all_features {
            cmd.arg("--all-features");
        } else {
            if self.features.no_default_features {
                cmd.arg("--no-default-features");
            }

            if !self.features.features.is_empty() {
                cmd.arg("--features").args(&self.features.features);
            }
        }

        cmd
    }

    pub fn crates_or_defaults<'cmd>(
        &'cmd self,
        defaults: &[&'cmd str],
        cmd: &'cmd mut Command,
    ) -> &'cmd mut Command {
        const PACKAGE: &str = "--package";
        if self.workspace.package.is_empty() {
            tracing::info!(crates = ?defaults, "Testing default crates");
            configure_cmd_list(PACKAGE, defaults.iter().copied(), cmd)
        } else if self.workspace.workspace {
            tracing::debug!("Testing workspace");
            cmd.arg("--workspace")
        } else {
            tracing::info!(crates = ?self.workspace.package, "Testing");
            configure_cmd_list(
                PACKAGE,
                self.workspace.package.iter().map(String::as_ref),
                cmd,
            )
        }
    }
}

fn configure_cmd_list<'c>(
    prefix: &'c str,
    vals: impl IntoIterator<Item = &'c str>,
    cmd: &'c mut Command,
) -> &'c mut Command {
    cmd.args(vals.into_iter().flat_map(|v| [prefix, v]))
}
