use crate::cli;
use color_eyre::eyre::{self, Result, WrapErr};
use std::process::Command;

impl cli::Options {
    pub fn cargo_cmd(&self, cmd: &str) -> Command {
        let mut cargo = Command::new(self.cargo_path.as_os_str());
        cargo
            .arg(cmd)
            // propagate our color mode configuration
            .env("CARGO_TERM_COLOR", self.output.color.as_str());
        cargo
    }

    pub fn has_nextest(&self) -> Result<bool> {
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
            Ok::<bool, eyre::Error>(stdout.contains("nextest"))
        })()
        .context("checking for `cargo nextest` failed")
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
