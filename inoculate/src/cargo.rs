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
    pub fn crates_or_defaults<'c>(
        &'c self,
        defaults: &[&'c str],
        cmd: &'c mut Command,
    ) -> &'c mut Command {
        fn configure_crates<'c>(
            cmd: &'c mut Command,
            crates: impl IntoIterator<Item = &'c str>,
        ) -> &'c mut Command {
            cmd.args(crates.into_iter().flat_map(|c| ["--package", c]))
        }

        if self.workspace.package.is_empty() {
            tracing::info!(crates = ?defaults, "Testing default crates");
            configure_crates(cmd, defaults.iter().copied())
        } else {
            tracing::info!(crates = ?self.workspace.package, "Testing");
            configure_crates(cmd, self.workspace.package.iter().map(String::as_ref))
        }
    }
}
