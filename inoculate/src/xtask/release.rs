use crate::{cli, Result};
use color_eyre::eyre::{self, WrapErr};

/// Arguments for a release command.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum Cmd {
    /// Update the changelog for `crate`, without tagging and publishing a new
    /// release.
    UpdateChangelog(ReleaseArgs),

    /// Tag and publish a new release of `crate`.
    ///
    /// This will also run the `update-changelog` command prior to releasing the crate.
    Release(ReleaseArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct ReleaseArgs {
    /// The name of the crate to release.
    #[clap(name = "crate")]
    crate_name: String,

    /// The git tag for the next release of this crate.
    tag: String,

    /// Whether or not to actually publish the release.
    #[clap(long)]
    dry_run: bool,
}

impl Cmd {
    pub(super) fn run(&self, opts: &cli::Options) -> Result<()> {
        match self {
            Cmd::UpdateChangelog(args) => args.update_changelog(opts).with_context(|| {
                format!("updating changelog for {} {}", args.crate_name, args.tag,)
            }),
            Cmd::Release(args) => {
                args.update_changelog(opts).with_context(|| {
                    format!("updating changelog for {} {}", args.crate_name, args.tag,)
                })?;
                args.release(opts)
                    .with_context(|| format!("releasing {} {}", args.crate_name, args.tag))
            }
        }
    }
}

impl ReleaseArgs {
    fn update_changelog(&self, opts: &cli::Options) -> Result<()> {
        tracing::info!(crate_name = %self.crate_name, tag = %self.tag, "updating changelog for");
        eyre::bail!("can't update changelog, not yet implemented")
    }

    fn release(&self, opts: &cli::Options) -> Result<()> {
        tracing::info!(crate_name = %self.crate_name, tag = %self.tag, "releasing");
        eyre::bail!("can't release, not yet implemented")
    }
}
