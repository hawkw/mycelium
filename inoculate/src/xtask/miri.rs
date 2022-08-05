use std::process::ExitStatus;

use crate::cli;
use color_eyre::{
    eyre::{Result, WrapErr},
    Help,
};

/// Options that configure Miri's behavior.
#[derive(Debug, clap::Args)]
#[clap(
    next_help_heading = "MIRI OPTIONS",
    group = clap::ArgGroup::new("miri-opts")
)]
pub(crate) struct MiriOptions {
    /// Maximum number of `proptest` cases to execute.
    ///
    /// This sets the value of the `ENV_PROPTEST_CASES` environment variable for
    /// the test executable.
    #[clap(long, env = ENV_PROPTEST_CASES, default_value_t = 10)]
    proptest_cases: usize,

    /// Flags to pass to Miri
    #[clap(long, env = ENV_MIRIFLAGS, default_value_t = String::from(DEFAULT_MIRIFLAGS))]
    miriflags: String,
}

const ENV_PROPTEST_CASES: &str = "PROPTEST_CASES";
const ENV_MIRIFLAGS: &str = "MIRIFLAGS";
const DEFAULT_MIRIFLAGS: &str = "-Zmiri-strict-provenance -Zmiri-disable-isolation";
const DEFAULT_CRATES: &[&str] = &["cordyceps", "mycelium-util"];

impl MiriOptions {
    pub(super) fn run(
        &self,
        opts: &cli::Options,
        cargo_opts: &cli::CargoOptions,
        extra_args: &[String],
    ) -> Result<ExitStatus> {
        tracing::info!("Running Miri tests");

        let mut cmd = opts.cargo_cmd("miri");
        if opts.install_nextest()? {
            cmd.args(["nextest", "run"]);
            if opts.ci {
                tracing::info!("Configuring nextest for CI");
                cmd.args(["--profile", "ci"]);
            } else {
                tracing::debug!("not configured for CI");
            };
        } else {
            cmd.arg("test");
        };

        cargo_opts
            .configure_with_crates(DEFAULT_CRATES, &mut cmd)
            .envs([
                (ENV_PROPTEST_CASES, self.proptest_cases.to_string()),
                (ENV_MIRIFLAGS, self.miriflags.clone()),
            ])
            .arg("--lib")
            .args(extra_args);
        tracing::debug!(?cmd, "running");
        cmd.status()
            .context("running Miri command failed")
            .with_note(|| format!("command: {:?}", cmd))
    }
}
