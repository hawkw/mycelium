use crate::{cli, Result};
use cargo_metadata::Package;
use color_eyre::{
    eyre::{self, WrapErr},
    Help,
};
use std::fs;

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
    #[clap(name = "CRATE")]
    crate_name: String,

    /// How to increment the crate's version for the release.
    ///
    /// The release can either be a major (X.0.0), minor (x.Y.0), or patch
    /// (x.y.Z) release.
    #[clap(value_enum)]
    version: Increment,

    /// Whether or not to actually publish the release.
    #[clap(long)]
    dry_run: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
enum Increment {
    /// Release a major version.
    Major,

    /// Release a minor version.
    Minor,

    /// Release a patch version.
    Patch,
}

impl Cmd {
    pub(super) fn run(&self, opts: &cli::Options) -> Result<()> {
        match self {
            Cmd::UpdateChangelog(args) => args.update_changelog(opts),
            Cmd::Release(args) => args.release(opts).with_note(|| {
                format!(
                    "while releasing {} (version={:?})",
                    args.crate_name, args.version
                )
            }),
        }
    }
}

impl ReleaseArgs {
    #[tracing::instrument(
        level = "info", skip(self, opts),
        fields(%self.crate_name, ?self.version, self.dry_run = self.dry_run)
    )]
    fn update_changelog(&self, opts: &cli::Options) -> Result<()> {
        (|| {
            tracing::info!(crate_name = %self.crate_name, version = ?self.version, "updating changelog");
            let metadata = metadata_for_crate(&self.crate_name, opts)?;
            let new_version = self.version.increment(&metadata.version);
            self.write_changelog_update(&metadata, new_version)

        })().with_context(|| {
            format!(
                "failed to update changelog for {}",
                self.crate_name,
            )
        })
    }

    fn write_changelog_update(
        &self,
        metadata: &Package,
        new_version: semver::Version,
    ) -> Result<()> {
        eyre::bail!("can't update changelog, not yet implemented")
    }

    #[tracing::instrument(
        level = "info", skip(self, opts),
        fields(%self.crate_name, ?self.version, self.dry_run = self.dry_run)
    )]
    fn release(&self, opts: &cli::Options) -> Result<()> {
        tracing::info!(crate_name = %self.crate_name, version = ?self.version, "releasing");
        let metadata = metadata_for_crate(&self.crate_name, opts)?;
        let new_version = self.version.increment(&metadata.version);
        self.update_cargo_toml(&metadata, &new_version)?;
        self.update_changelog(opts)?;
        eyre::bail!("can't release, not yet implemented")
    }

    #[tracing::instrument]
    fn update_cargo_toml(&self, metadata: &Package, version: &semver::Version) -> Result<()> {
        let old_version = &metadata.version;
        let cargo_toml_path = metadata.manifest_path.as_path();
        tracing::info!(
            new_version = %version,
            %old_version,
            "Updating {cargo_toml_path}"
        );
        let file = fs::read_to_string(cargo_toml_path).context("failed to read Cargo.toml")?;

        let new_file = file.replace(
            &format!("version = \"{}\"", old_version),
            &format!("version = \"{}\"", version),
        );

        prettydiff::text::diff_lines(&file, &new_file)
            .names("old Cargo.toml", "new Cargo.toml")
            .set_diff_only(true)
            .prettytable();

        if self.dry_run {
            tracing::info!("dry run: skipping Cargo.toml update");
            return Ok(());
        }

        fs::write(cargo_toml_path, new_file).context("failed to write Cargo.toml")?;
        tracing::info!("updated {cargo_toml_path}");
        Ok(())
    }
}

#[tracing::instrument(skip(opts))]
fn metadata_for_crate(crate_name: &str, opts: &cli::Options) -> Result<cargo_metadata::Package> {
    (|| {
        let cmd = opts.metadata_command();
        tracing::debug!(command = ?cmd.cargo_command(), "running");

        let metadata = cmd
            .exec()
            .with_context(|| format!("failed to execute {cmd:?}"))?;

        for pkg in metadata.packages {
            if pkg.name == crate_name {
                tracing::trace!(%crate_name, ?pkg, "found package");
                return Ok(pkg);
            }
        }

        Err(eyre::format_err!(
            "could not find a package for {crate_name}"
        ))
    })()
    .with_note(|| format!("while getting metadata for {crate_name}"))
}

// === impl Increment ===

impl Increment {
    fn increment(&self, current: &semver::Version) -> semver::Version {
        let next = match self {
            Increment::Major => semver::Version::new(current.major + 1, 0, 0),
            Increment::Minor => semver::Version::new(current.major, current.minor + 1, 0),
            Increment::Patch => {
                semver::Version::new(current.major, current.minor, current.patch + 1)
            }
        };
        tracing::info!(%current, %next, "Incremented version");
        next
    }
}
