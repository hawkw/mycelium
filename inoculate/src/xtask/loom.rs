use std::{
    path::PathBuf,
    process::{Command, ExitStatus},
};

use crate::cli;
use color_eyre::{
    eyre::{Result, WrapErr},
    Help,
};

/// Options that configure Loom's behavior.
#[derive(Debug, clap::Args)]
#[clap(
    next_help_heading = "LOOM OPTIONS",
    group = clap::ArgGroup::new("loom-opts")
)]
pub(crate) struct LoomOptions {
    /// Maximum number of thread switches per permutation.
    ///
    /// This sets the value of the `LOOM_MAX_BRANCHES` environment variable for
    /// the test executable.
    #[clap(long, env = ENV_MAX_BRANCHES, default_value_t = 1_000)]
    max_branches: usize,

    /// Maximum number of permutations to explore
    ///
    /// If no value is provided, the number of permutations will not be bounded.
    ///
    /// This sets the value of the `LOOM_MAX_PERMUTATIONS` environment variable
    /// for the test executable.
    #[clap(long, env = ENV_MAX_PERMUTATIONS, default_value_t = 2)]
    max_permutations: usize,

    /// Maximum number of thread preemptions to explore
    ///
    /// If no value is provided, the number of thread preemptions will not be
    /// bounded.
    ///
    /// This sets the value of the `LOOM_MAX_PREEMPTIONS` environment variable
    /// for the test executable.
    #[clap(long, env = ENV_MAX_PREEMPTIONS)]
    max_preemptions: Option<usize>,

    /// Max number of threads to check as part of the execution.
    ///
    /// This should be set as low as possible and must be less than 4.
    ///
    /// This sets the value of the `LOOM_MAX_THREADS` environment variable for
    /// the test execution.
    #[clap(long, env = ENV_MAX_THREADS, default_value_t = 4)]
    max_threads: usize,

    /// How often to write the checkpoint file
    ///
    /// This sets the value of the `LOOM_CHECKPOINT_INTERVAL` environment
    /// variable for the test executable.
    ///
    /// If no value is provided, checkpoints will be written every 5 iterations.
    #[clap(long, env = ENV_CHECKPOINT_INTERVAL, default_value_t = 5)]
    checkpoint_interval: usize,

    /// Path for the checkpoint file.
    ///
    /// This sets the value of the `LOOM_CHECKPOINT_FILE` environment
    /// variable for the test executable.
    ///
    /// If no value is provided, the checkpoint file will not be written.
    #[clap(long, env = ENV_CHECKPOINT_FILE)]
    checkpoint_file: Option<PathBuf>,

    /// Maximum duration to run each loom model for, in seconds
    ///
    /// If a value is not provided, no duration limit will be set.
    ///
    /// This sets the value of the `LOOM_MAX_DURATION` environment variable for
    /// the test executable.
    #[clap(long, env = ENV_MAX_DURATION)]
    max_duration_secs: Option<usize>,

    /// Log level filter for `loom` when re-running failed tests
    #[clap(long, env = ENV_LOOM_LOG, default_value = "trace")]
    loom_log: String,
}

const DEFAULT_CRATES: &[&str] = &["cordyceps", "maitake", "mycelium-util"];

const ENV_CHECKPOINT_INTERVAL: &str = "LOOM_CHECKPOINT_INTERVAL";
const ENV_MAX_BRANCHES: &str = "LOOM_MAX_BRANCHES";
const ENV_MAX_DURATION: &str = "LOOM_MAX_DURATION";
const ENV_MAX_PERMUTATIONS: &str = "LOOM_MAX_PERMUTATIONS";
const ENV_MAX_PREEMPTIONS: &str = "LOOM_MAX_PREEMPTIONS";
const ENV_MAX_THREADS: &str = "LOOM_MAX_THREADS";
const ENV_LOOM_LOG: &str = "LOOM_LOG";
const ENV_CHECKPOINT_FILE: &str = "LOOM_CHECKPOINT_FILE";
// const ENV_LOOM_LOCATION: &str = "LOOM_LOCATION";

impl LoomOptions {
    pub(super) fn run(
        &self,
        opts: &cli::Options,
        cargo_opts: &cli::CargoOptions,
        extra_args: &[String],
    ) -> Result<ExitStatus> {
        tracing::info!("Running Loom tests");

        let mut cmd = if opts.install_nextest()? {
            let mut cmd = opts.cargo_cmd("nextest");
            let nextest_profile = if opts.ci {
                tracing::info!("Configured for CI");
                "loom-ci"
            } else {
                tracing::debug!("not configured for CI");
                "loom"
            };
            cmd.args([
                "run",
                "--cargo-profile",
                "loom",
                "--profile",
                nextest_profile,
            ]);
            cmd
        } else {
            tracing::info!("Nextest not found, using 'cargo test'");
            let mut cmd = opts.cargo_cmd("test");
            cmd.args(["--profile", "loom"]);
            cmd
        };

        cargo_opts.configure_with_crates(DEFAULT_CRATES, &mut cmd);
        self.configure_cmd(&mut cmd).args(extra_args);

        tracing::debug!(?cmd, "running");
        cmd.status()
            .context("running Loom command failed")
            .with_note(|| format!("command: {:?}", cmd))
    }

    fn configure_cmd<'cmd>(&self, cmd: &'cmd mut Command) -> &'cmd mut Command {
        cmd.envs([
            (ENV_MAX_BRANCHES, self.max_branches.to_string()),
            (ENV_MAX_THREADS, self.max_threads.to_string()),
            (ENV_MAX_PERMUTATIONS, self.max_permutations.to_string()),
            (
                ENV_CHECKPOINT_INTERVAL,
                self.checkpoint_interval.to_string(),
            ),
        ])
        .envs([
            (ENV_LOOM_LOG, self.loom_log.as_str()),
            ("RUSTFLAGS", "--cfg loom"),
        ]);

        if let Some(max_dur) = self.max_duration_secs {
            cmd.env(ENV_MAX_DURATION, max_dur.to_string());
        }

        if let Some(max_preemptions) = self.max_preemptions {
            cmd.env(ENV_MAX_PREEMPTIONS, max_preemptions.to_string());
        }

        if let Some(ref checkpoint_file) = self.checkpoint_file {
            cmd.env(ENV_CHECKPOINT_FILE, checkpoint_file);
            cmd.env(
                ENV_CHECKPOINT_INTERVAL,
                self.checkpoint_interval.to_string(),
            );
        }

        cmd.arg("--lib")
    }
}
