use crate::{cli, Result};

mod loom;
mod miri;

#[derive(Debug, clap::Parser)]
pub(crate) enum Cmd {
    /// Run `loom` tests.
    Loom {
        #[clap(flatten)]
        loom_opts: loom::LoomOptions,
    },
    /// Run tests using the Miri interpreter.
    Miri {
        #[clap(flatten)]
        miri_opts: miri::MiriOptions,
    },
    /// Run `loom` tests, Miri tests, and host tests.
    AllTests {
        #[clap(flatten)]
        loom_opts: loom::LoomOptions,

        #[clap(flatten)]
        miri_opts: miri::MiriOptions,
    },
}

impl Cmd {
    pub(crate) fn run(&self, opts: &cli::Options, cargo_opts: &cli::CargoOptions) -> Result<()> {
        match self {
            Self::Loom { loom_opts } => {
                let status = loom_opts.run(opts, cargo_opts)?;
                if !status.success() {
                    tracing::error!(status = status.code(), "running Loom tests failed");
                    std::process::exit(status.code().unwrap_or(1));
                };

                Ok(())
            }
            Self::AllTests {
                loom_opts,
                miri_opts,
            } => {
                let loom_status = loom_opts.run(opts, cargo_opts);
                let miri_status = miri_opts.run(opts, cargo_opts);

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
            Self::Miri { miri_opts } => {
                let status = miri_opts.run(opts, cargo_opts)?;
                if !status.success() {
                    tracing::error!(status = status.code(), "running Miri tests failed");
                    std::process::exit(status.code().unwrap_or(1));
                };

                Ok(())
            }
        }
    }
}
