use clap::Parser;
use color_eyre::{eyre::WrapErr, Help};
use std::path::PathBuf;

pub use color_eyre::eyre::Result;
pub mod cargo;
pub mod cli;
pub mod term;
pub mod trace;

mod inoculate;
mod xtask;

#[derive(Debug, Parser)]
#[clap(
    bin_name = "cargo",
    about,
    version,
    author = "Eliza Weisman <eliza@elizas.website>",
    propagate_version = true
)]
pub struct Args {
    /// Which command to run?
    ///
    /// By default, an image is built but not run.
    #[clap(subcommand)]
    pub(crate) cmd: Cmd,
}

#[derive(Debug, Parser)]
pub(crate) enum Cmd {
    /// the horrible Mycelium build tool (because that's a thing we have to have
    /// now, apparently!)
    ///
    /// Run an `inoculate` command that builds the Mycelium kernel. This is
    /// intended to be invoked via `cargo run` commands with a runner alias.
    #[clap(
        version,
        author = "Eliza Weisman <eliza@elizas.website>",
        propagate_version = true
    )]
    Inoculate {
        /// The path to the kernel binary.
        #[clap(parse(from_os_str))]
        kernel_bin: PathBuf,

        #[clap(flatten)]
        paths: cli::PathOptions,

        /// Which command to run?
        ///
        /// By default, an image is built but not run.
        #[clap(subcommand)]
        cmd: Option<inoculate::Cmd>,

        #[clap(flatten)]
        options: cli::Options,
    },

    /// Runs a Mycelium `cargo xtask`.
    ///
    /// These automate various workflows for testing, linting, and building
    /// Mycelium.
    #[clap(
        version,
        author = "Eliza Weisman <eliza@elizas.website>",
        propagate_version = true
    )]
    Xtask {
        #[clap(flatten)]
        options: cli::Options,

        #[clap(flatten)]
        cargo_options: cli::CargoOptions,

        /// Which `xtask` command to run?
        #[clap(subcommand)]
        cmd: xtask::Cmd,
    },
}

impl Args {
    pub fn init_term(&mut self) -> Result<&mut Self> {
        self.options_mut().init_term()?;
        Ok(self)
    }

    pub fn is_test(&self) -> bool {
        match self.cmd {
            Cmd::Inoculate {
                cmd: Some(ref cmd), ..
            } => cmd.is_test(),
            _ => false,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        match self.cmd {
            Cmd::Xtask {
                ref options,
                ref cargo_options,
                ref cmd,
            } => cmd.run(options, cargo_options),
            Cmd::Inoculate {
                ref kernel_bin,
                ref paths,
                ref cmd,
                ref options,
            } => {
                tracing::info!("inoculating mycelium!");
                tracing::trace!(
                    ?cmd,
                    paths.kernel_bin = ?kernel_bin,
                    ?paths.bootloader_manifest,
                    ?paths.kernel_manifest,
                    ?paths.target_dir,
                    ?paths.out_dir,
                    "inoculate configuration"
                );
                let paths = paths.paths(kernel_bin)?;

                let image = paths
                    .make_image(options)
                    .context("making the mycelium image didnt work")
                    .note("this sucks T_T")?;

                if let Some(subcmd) = cmd {
                    subcmd.run(image, &paths)?;
                }

                Ok(())
            }
        }
    }

    fn options_mut(&mut self) -> &mut cli::Options {
        match self.cmd {
            Cmd::Inoculate {
                ref mut options, ..
            } => options,
            Cmd::Xtask {
                ref mut options, ..
            } => options,
        }
    }
}
