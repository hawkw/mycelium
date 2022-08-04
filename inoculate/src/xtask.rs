use crate::{cli, Result};

mod loom;
mod miri;

#[derive(Debug, clap::Parser)]
#[clap(trailing_var_arg = true)]
pub(crate) enum Cmd {
    /// Run Loom tests.
    ///
    /// This command uses `cargo nextest` to run Loom tests in the specified
    /// crates, or a default list of crates that have Loom tests, if no crates
    /// are specified.
    Loom {
        #[clap(flatten)]
        loom_opts: loom::LoomOptions,

        #[clap(flatten)]
        cargo_opts: cli::CargoOptions,

        /// Extra arguments passed to the `cargo nextest` invocation.
        nextest_args: Vec<String>,
    },

    /// Run tests using the Miri interpreter.
    ///
    /// This command uses `cargo nextest` to run Miri tests in the specified
    /// crates, or a default list of crates that have Loom tests, if no crates
    /// are specified.
    Miri {
        #[clap(flatten)]
        miri_opts: miri::MiriOptions,

        #[clap(flatten)]
        cargo_opts: cli::CargoOptions,

        /// Extra arguments passed to the `cargo nextest` invocation.
        nextest_args: Vec<String>,
    },

    /// Run `loom` tests, Miri tests, and host tests.
    AllTests {
        #[clap(flatten)]
        loom_opts: loom::LoomOptions,

        #[clap(flatten)]
        miri_opts: miri::MiriOptions,

        #[clap(flatten)]
        cargo_opts: cli::CargoOptions,
    },
}

impl Cmd {
    pub(crate) fn run(&self, opts: &cli::Options) -> Result<()> {
        match self {
            Self::Loom {
                loom_opts,
                cargo_opts,
                nextest_args,
            } => {
                let status = loom_opts.run(opts, cargo_opts, &nextest_args[..])?;
                if !status.success() {
                    tracing::error!(status = status.code(), "running Loom tests failed");
                    std::process::exit(status.code().unwrap_or(1));
                };

                Ok(())
            }
            Self::AllTests {
                loom_opts,
                cargo_opts,
                miri_opts,
            } => {
                let loom_status = loom_opts.run(opts, cargo_opts, &[]);
                let miri_status = miri_opts.run(opts, cargo_opts, &[]);

                let loom_status = loom_status?;
                let miri_status = miri_status?;

                let mut failed = false;

                if !loom_status.success() {
                    tracing::error!(status = loom_status.code(), "running Loom tests failed");
                    failed = true;
                }

                if !miri_status.success() {
                    tracing::error!(status = miri_status.code(), "running Miri tests failed");
                    failed = true;
                }

                if failed {
                    std::process::exit(1);
                }

                Ok(())
            }
            Self::Miri {
                miri_opts,
                cargo_opts,
                nextest_args,
            } => {
                let status = miri_opts.run(opts, cargo_opts, &nextest_args[..])?;
                if !status.success() {
                    tracing::error!(status = status.code(), "running Miri tests failed");
                    std::process::exit(status.code().unwrap_or(1));
                };

                Ok(())
            }
        }
    }
}
