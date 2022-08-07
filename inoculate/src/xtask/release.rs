use crate::{cli, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::Package;
use color_eyre::{
    eyre::{self, ensure, WrapErr},
    Help,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

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

    /// The path to the crate's changelog. If this isn't provided, the changelog
    /// is assumed to be at `{crate}/CHANGELOG.md`.
    #[clap(long = "changelog")]
    changelog_path: Option<Utf8PathBuf>,

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
            let tag = format!("{}-v{}", metadata.name, new_version);
            self.write_changelog_update(&metadata, &tag, opts).map(|_|())
        })().with_context(|| {
            format!(
                "failed to update changelog for {}",
                self.crate_name,
            )
        })
    }

    #[tracing::instrument(
        level = "info", skip(self, opts, metadata),
        fields(
            %self.crate_name,
            ?self.version,
            self.dry_run = self.dry_run,
            ?self.changelog_path,
        )
    )]
    fn write_changelog_update(
        &self,
        metadata: &Package,
        tag: &str,
        opts: &cli::Options,
    ) -> Result<Utf8PathBuf> {
        let repo_dir = opts
            .manifest
            .manifest_path
            .as_deref()
            .and_then(Path::parent)
            .map(PathBuf::from)
            .ok_or(())
            .or_else(|_| std::env::current_dir())?;

        let pkg_dir = metadata
            .manifest_path
            .as_path()
            .parent()
            .ok_or_else(|| eyre::format_err!("metadata path should have a parent"))?;
        let changelog_path = self.changelog_path.clone().unwrap_or_else(|| {
            tracing::debug!("no changelog path specified, using default");
            pkg_dir.join("CHANGELOG.md")
        });
        let changelog_exists = changelog_path.exists();

        tracing::debug!(%changelog_path, changelog_exists);
        if !changelog_exists {
            tracing::info!(
                "changelog for `{}` does not exist, creating it",
                self.crate_name
            );
            fs::File::create(&changelog_path)
                .context("failed to create changelog file")
                .with_note(|| format!("path: {changelog_path}"))?;
        }

        let git_cliff_opts = {
            let include =
                format!("{}/**", pkg_dir.file_name().expect("must have a file name")).parse()?;
            tracing::debug!(%include);
            let pkg_dir = pkg_dir.as_std_path();
            git_cliff::args::Opt {
                verbose: 255,
                config: repo_dir.join(".config").join("cliff.toml"),
                workdir: Some(pkg_dir.to_path_buf()),
                repository: Some(repo_dir),
                include_path: Some(vec![include]),
                exclude_path: None,
                prepend: None,
                output: Some(changelog_path.as_std_path().to_path_buf()),
                tag: Some(tag.to_string()),
                body: None,
                with_commit: None,
                init: false,
                latest: true,
                unreleased: true,
                current: true,
                context: false,
                date_order: true,
                sort: git_cliff::args::Sort::Oldest,
                range: None,
                strip: None,
            }
        };
        git_cliff::run(git_cliff_opts).context("running `git-cliff` failed")?;
        tracing::info!(%tag, "Updated changelog `{}`", changelog_path);
        Ok(changelog_path)
    }

    #[tracing::instrument(
        level = "info", skip(self, opts),
        fields(%self.crate_name, ?self.version, self.dry_run = self.dry_run)
    )]
    fn release(&self, opts: &cli::Options) -> Result<()> {
        tracing::info!(crate_name = %self.crate_name, version = ?self.version, "releasing");
        let metadata = metadata_for_crate(&self.crate_name, opts)?;
        let new_version = self.version.increment(&metadata.version);
        let tag = format!("{}-v{}", metadata.name, new_version);
        self.verify_release(&tag, opts).with_context(|| {
            format!(
                "failed to verify if {} {} could be released",
                self.crate_name, tag
            )
        })?;
        let cargo_toml_path = self.update_cargo_toml(&metadata, &new_version)?;
        let changelog_path = self
            .write_changelog_update(&metadata, &tag, opts)
            .with_context(|| {
                format!(
                    "updating changelog for {crate_name} {new_version}",
                    crate_name = &self.crate_name
                )
            })?;
        let paths = &[cargo_toml_path, changelog_path.as_path()];
        let mut git_add = Command::new("git");
        let status = git_add.arg("add").args(paths).status()?;
        ensure!(status.success(), "running {git_add:?} failed");

        tracing::info!("ready to prepare release commit");
        Command::new("git").args(["diff", "--staged"]).status()?;

        if self.dry_run {
            Command::new("git")
                .args(["reset", "HEAD", "--"])
                .args(paths)
                .status()?;
            Command::new("git")
                .args(["checkout", "HEAD", "--"])
                .args(paths)
                .status()?;
        } else {
            if !opts.confirm("commit and push?") {
                tracing::info!("Push skipped, exiting");
                return Ok(());
            }

            Command::new("git")
                .args([
                    "commit",
                    "-s",
                    "-S",
                    "-m",
                    format!(
                        "chore({crate_name}): prepare to release {crate_name} v{new_version}",
                        crate_name = self.crate_name,
                    )
                    .as_str(),
                ])
                .args(paths)
                .status()?;
        }

        Ok(())
    }

    #[tracing::instrument(
        level = "info", skip(self, opts),
        fields(
            %self.crate_name,
            ?self.version,
            ?self.changelog_path,
        )
    )]
    fn verify_release(&self, tag: &str, opts: &cli::Options) -> Result<()> {
        tracing::info!("verifying if `{}` {} can be released", self.crate_name, tag);
        if opts.install_subcommand("hack")? {
            tracing::info!(
                "checking if `{}` builds across feature combinations",
                self.crate_name
            );
            let mut cmd = opts.cargo_cmd("hack");
            cmd.args([
                "--package",
                self.crate_name.as_str(),
                "--feature-powerset",
                "--no-dev-deps",
                "check",
                "--quiet",
            ]);
            tracing::debug!(?cmd, "running");
            let status = cmd
                .status()
                .with_context(|| format!("running {cmd:?} failed"))?;
            if !status.success() {
                tracing::error!(crate_name = %self.crate_name, code = ?status.code(), "`cargo-hack` did not exit successfully");
                process::exit(status.code().unwrap_or(1));
            }
        } else {
            tracing::warn!("cargo-hack must be installed to verify a release, skipping");
        }

        let mut cmd = Command::new("git");
        cmd.args(["tag", "-l", tag]);
        tracing::debug!(?cmd, "running");
        let stdout = cmd
            .output()
            .with_context(|| format!("running {cmd:?} failed"))?
            .stdout;
        let stdout =
            String::from_utf8(stdout).context("failed to parse `git tag` stdout as utf8")?;
        if stdout == tag {
            tracing::error!("git tag `{tag}` already exists, cannot release");
            process::exit(1);
        }

        Ok(())
    }

    #[tracing::instrument]
    fn update_cargo_toml<'p>(
        &self,
        metadata: &'p Package,
        version: &semver::Version,
    ) -> Result<&'p Utf8Path> {
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
            return Ok(cargo_toml_path);
        }

        fs::write(cargo_toml_path, new_file).context("failed to write Cargo.toml")?;
        tracing::info!("updated {cargo_toml_path}");
        Ok(cargo_toml_path)
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
                tracing::debug!(%crate_name, %pkg.id, "found package");
                tracing::trace!(package = ?format_args!("{pkg:#?}"));
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
