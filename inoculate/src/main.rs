use color_eyre::{
    eyre::{ensure, format_err, Result, WrapErr},
    Help, SectionExt,
};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::Duration,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "inoculate",
    about = "the horrible mycelium build tool (because that's a thing we have to have now apparently!)"
)]
struct Options {
    /// Which command to run?
    ///
    /// By default, an image is built but not run.
    #[structopt(subcommand)]
    cmd: Option<RunCmd>,

    /// Configures build logging.
    #[structopt(short, long, env = "RUST_LOG", default_value = "warn")]
    log: String,

    /// The path to the kernel binary.
    #[structopt(parse(from_os_str))]
    kernel_bin: PathBuf,

    /// The path to the `bootloader` crate's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[structopt(long, parse(from_os_str))]
    bootloader_manifest: Option<PathBuf>,

    /// The path to the kernel's Cargo manifest. If this is not
    /// provided, it will be located automatically.
    #[structopt(long, parse(from_os_str))]
    kernel_manifest: Option<PathBuf>,

    /// Overrides the directory in which to build the output image.
    #[structopt(short, long, parse(from_os_str))]
    out_dir: Option<PathBuf>,

    /// Overrides the target directory for the kernel build.
    #[structopt(short, long, parse(from_os_str))]
    target_dir: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
enum RunCmd {
    /// Builds a bootable disk image and runs it in QEMU (implies: `build`).
    Run {
        /// Redirect the VM's serial output to stdout
        #[structopt(long, short)]
        serial: bool,

        /// Extra arguments passed to QEMU
        #[structopt(flatten)]
        qemu_settings: QemuSettings,
    },
    /// Builds a bootable disk image with tests enabled, and runs the tests in QEMU.
    Test {
        /// Timeout for failing test run, in seconds.
        ///
        /// If a test run doesn't complete before this timeout has elapsed, it's
        /// considered to have failed.
        #[structopt(long, short, parse(try_from_os_str = parse_secs), default_value = "60")]
        timeout_secs: Duration,

        /// Disables capturing test serial output.
        #[structopt(long)]
        nocapture: bool,

        /// Extra arguments passed to QEMU
        #[structopt(flatten)]
        qemu_settings: QemuSettings,
    },
}

#[derive(Debug, StructOpt)]
struct QemuSettings {
    /// Listen for GDB connections on TCP port 1234
    #[structopt(long, short)]
    debug: bool,

    /// The TCP port to listen for debug connections on.
    #[structopt(long, default_value = "1234")]
    debug_port: u16,

    /// Extra arguments passed to QEMU
    #[structopt(raw = true)]
    qemu_args: Vec<String>,
}

impl Options {
    fn wheres_bootloader(&self) -> Result<PathBuf> {
        tracing::debug!("where's bootloader?");
        if let Some(path) = self.bootloader_manifest.as_ref() {
            tracing::info!(path = %path.display(), "bootloader path overridden");
            return Ok(path.clone());
        }
        bootloader_locator::locate_bootloader("bootloader")
            .note("where the hell is the `bootloader` crate's Cargo.toml?")
            .suggestion("maybe you forgot to depend on it")
    }

    fn wheres_the_kernel(&self) -> Result<PathBuf> {
        tracing::debug!("where's the kernel?");
        if let Some(path) = self.kernel_manifest.as_ref() {
            tracing::info!(path = %path.display(), "kernel manifest path path overridden");
            return Ok(path.clone());
        }
        locate_cargo_manifest::locate_manifest()
            .note("where the hell is the kernel's Cargo.toml?")
            .note("this should never happen, seriously, wtf")
            .suggestion("have you tried not having it be missing")
    }

    fn make_image(&self, bootloader_manifest: &Path, kernel_manifest: &Path) -> Result<PathBuf> {
        let _span = tracing::info_span!("make_image").entered();
        let kernel_bin = self
            .kernel_bin
            // the bootloader crate's build script gets mad if this is a
            // relative pathe
            .canonicalize()
            .context("couldn't to canonicalize kernel manifest path")
            .note("it should work")?;
        tracing::debug!(kernel_bin = %kernel_bin.display(), "making boot image...");
        let out_dir = self
            .out_dir
            .as_ref()
            .map(|path| path.as_ref())
            .or_else(|| kernel_bin.parent())
            .ok_or_else(|| format_err!("can't find out dir, wtf"))
            .context("determining out dir")
            .note("somethings messed up lol")?;
        let target_dir = self
            .target_dir
            .clone()
            .or_else(|| Some(kernel_manifest.parent()?.join("target")))
            .ok_or_else(|| format_err!("can't find target dir, wtf"))
            .context("determining target dir")
            .note("somethings messed up lol")?;
        let run_dir = bootloader_manifest
            .parent()
            .ok_or_else(|| format_err!("bootloader manifest path doesn't have a parent dir"))
            .note("thats messed up lol")
            .suggestion("maybe dont run this in `/`???")?;
        let output = Command::new(env!("CARGO"))
            .current_dir(run_dir)
            .arg("builder")
            .arg("--kernel-manifest")
            .arg(&kernel_manifest)
            .arg("--kernel-binary")
            .arg(&kernel_bin)
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--target-dir")
            .arg(target_dir)
            .status()
            .context("run builder command")?;
        // TODO(eliza): modes for capturing/piping stdout?

        // let stdout = String::from_utf8_lossy(&output.stdout);
        // if !output.status.success() {
        if !output.success() {
            // let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format_err!(
                "bootloader's builder command exited with non-zero status code"
            ))
            // .with_section(move || stdout.trim().to_string().header("stdout:"))
            // .with_section(move || stderr.trim().to_string().header("stderr:"))
            .suggestion("if you had gotten all the inputs right, this should have worked");
        }

        let bin_name = self.kernel_bin.file_name().unwrap().to_str().unwrap();
        let image = out_dir.join(format!("boot-bios-{}.img", bin_name));
        ensure!(
            image.exists(),
            "disk image should probably exist after running bootloader build command"
        );

        Ok(image)
    }
}

impl RunCmd {
    #[tracing::instrument(skip(self))]
    fn run_qemu(&self, image: &Path) -> Result<()> {
        // TODO(eliza): should we `which qemu` here?
        let mut qemu = Command::new("qemu-system-x86_64");
        qemu.arg("-drive")
            .arg(format!("format=raw,file={}", image.display()))
            .arg("--no-reboot")
            .arg("-s");

        match self {
            RunCmd::Run {
                serial,
                qemu_settings,
            } => {
                tracing::info!("running QEMU in normal mode");
                if *serial {
                    tracing::debug!("configured QEMU to output serial on stdio");
                    qemu.arg("-serial").arg("stdio");
                }

                qemu_settings.configure(&mut qemu);
                qemu.arg("--no-shutdown");

                // Run qemu
                let out = qemu.output()?;
                if out.status.success() {
                    Ok(())
                } else {
                    let stdout = String::from_utf8_lossy(&out.stdout);

                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let status = out.status.code();
                    Err(format_err!("qemu exited with a non-zero status code"))
                        .with_section(move || format!("{:?}", status).header("status code:"))
                        .with_section(move || stdout.trim().to_string().header("stdout:"))
                        .with_section(move || stderr.trim().to_string().header("stderr:"))
                }
            }

            RunCmd::Test {
                qemu_settings,
                timeout_secs,
                nocapture,
            } => {
                use wait_timeout::ChildExt;

                // TODO(eliza):
                const TEST_ARGS: &[&str] = &[
                    "-device",
                    "isa-debug-exit,iobase=0xf4,iosize=0x04",
                    "-display",
                    "none",
                    "-serial",
                    "stdio",
                ];

                tracing::info!("running QEMU in test mode");
                qemu_settings.configure(&mut qemu);
                qemu.args(TEST_ARGS);

                let (mut child, stdout) = if !*nocapture {
                    tracing::debug!("capturing QEMU stdout");
                    let mut child = qemu
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .context("spawning QEMU with captured stdout failed")?;
                    let mut stdout = child.stdout.take().expect("wtf");
                    let stdout = std::thread::spawn(move || {
                        use std::io::Read;
                        let mut output = String::new();
                        stdout
                            .read_to_string(&mut output)
                            .map(move |_| output)
                            .context("reading QEMU stdout failed")
                    });
                    (child, Some(stdout))
                } else {
                    let child = qemu
                        .spawn()
                        .context("spawning QEMU without captured stdout failed")?;
                    (child, None)
                };

                let res = match child
                    .wait_timeout(*timeout_secs)
                    .context("waiting for QEMU to complete failed")?
                {
                    None => child
                        .kill()
                        .map_err(Into::into)
                        .and_then(|_| {
                            child
                                .wait()
                                .context("waiting for QEMU process to complete failed")
                        })
                        .context("killing QEMU process failed")
                        .and_then(|status: ExitStatus| {
                            Err(format_err!("test QEMU process exited with {}", status))
                        })
                        .with_context(|| format!("tests timed out after {:?}", *timeout_secs))
                        .note("maybe the kernel hung or boot looped?"),
                    Some(status) => {
                        if let Some(code) = status.code() {
                            if code == 33 {
                                println!("tests completed successfully");
                                return Ok(());
                            }

                            Err(format_err!("QEMU exited with status code {}", code))
                        } else {
                            Err(format_err!("QEMU exited without a status code, wtf?"))
                        }
                    }
                }
                .context("tests failed");
                if let Some(stdout) = stdout {
                    tracing::trace!("collecting stdout");
                    let stdout = stdout.join().unwrap()?;
                    tracing::trace!(?stdout);
                    res.with_section(move || stdout.trim().to_string().header("serial output:"))
                } else {
                    res
                }
            }
        }
    }
}

impl QemuSettings {
    fn configure(&self, cmd: &mut Command) {
        if self.debug {
            tracing::debug!(
                debug_port = self.debug_port,
                "configured QEMU to wait for GDB"
            );
            cmd.arg("-s")
                .arg("--gdb")
                .arg(format!("tcp::{}", self.debug_port));
        }

        cmd.args(&self.qemu_args[..]);
    }
}

fn parse_secs(s: &OsStr) -> std::result::Result<Duration, OsString> {
    s.to_str()
        .ok_or_else(|| OsString::from(s))
        .and_then(|s| s.parse::<u64>().map_err(|_| OsString::from(s)))
        .map(Duration::from_secs)
}

fn main() -> Result<()> {
    use tracing_subscriber::prelude::*;

    color_eyre::install()?;
    let opts = Options::from_args();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_error::ErrorLayer::default())
        .with(opts.log.parse::<tracing_subscriber::EnvFilter>()?)
        .init();

    tracing::info! {
        ?opts.cmd,
        ?opts.kernel_bin,
        ?opts.bootloader_manifest,
        ?opts.kernel_manifest,
        ?opts.target_dir,
        ?opts.out_dir,
        "inoculating...",
    };
    let bootloader_manifest = opts.wheres_bootloader()?;
    tracing::info!(path = %bootloader_manifest.display(), "found bootloader manifest");

    let kernel_manifest = opts.wheres_the_kernel()?;
    tracing::info!(path = %kernel_manifest.display(), "found kernel manifest");

    let image = opts
        .make_image(bootloader_manifest.as_ref(), kernel_manifest.as_ref())
        .context("making the mycelium image didnt work")
        .note("this sucks T_T")?;
    tracing::info!(image = %image.display());

    if let Some(run_cmd) = opts.cmd {
        return run_cmd.run_qemu(image.as_ref());
    }

    Ok(())
}
