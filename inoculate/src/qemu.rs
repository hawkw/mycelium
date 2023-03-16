use crate::term::{ColorMode, OwoColorize};
use crate::Result;
use color_eyre::{
    eyre::{ensure, format_err, WrapErr},
    Help, SectionExt,
};
use mycotest::TestName;
use std::{
    collections::BTreeMap,
    fmt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Builds a bootable disk image and runs it in QEMU (implies: `build`).
    Run {
        /// Redirect the VM's serial output to stdout
        #[clap(long, short)]
        serial: bool,

        /// Extra arguments passed to QEMU
        #[clap(flatten)]
        qemu_settings: Settings,
    },
    /// Builds a bootable disk image with tests enabled, and runs the tests in QEMU.
    Test {
        /// Timeout for failing test run, in seconds.
        ///
        /// If a test run doesn't complete before this timeout has elapsed, it's
        /// considered to have failed.
        #[clap(long, value_parser = parse_secs, default_value = "60")]
        timeout_secs: Duration,

        /// Disables capturing test serial output.
        #[clap(long)]
        nocapture: bool,

        /// Show captured serial output of successful tests
        #[clap(long)]
        show_output: bool,

        /// Extra arguments passed to QEMU
        #[clap(flatten)]
        qemu_settings: Settings,
    },
}

#[derive(Debug, clap::Args)]
pub struct Settings {
    /// Listen for GDB connections.
    #[clap(long, short)]
    gdb: bool,

    /// The TCP port to listen for debug connections on.
    #[clap(long, default_value = "1234")]
    gdb_port: u16,

    /// Extra arguments passed to QEMU
    #[clap(raw = true)]
    qemu_args: Vec<String>,
}

#[derive(Debug)]
struct TestResults {
    tests: usize,
    completed: usize,
    failed: BTreeMap<TestName<'static, String>, Vec<String>>,
    panicked: usize,
    faulted: usize,
    total: usize,
}

impl Cmd {
    fn should_capture(&self) -> bool {
        match self {
            Cmd::Test {
                nocapture: true, ..
            } => {
                tracing::debug!("running tests with `--nocapture`, will not capture.");
                false
            }
            Cmd::Run { serial: true, .. } => {
                tracing::debug!("running normally with `--serial`, will not capture");
                false
            }
            Cmd::Run { qemu_settings, .. }
                if qemu_settings
                    .qemu_args
                    .iter()
                    .map(String::as_str)
                    .any(|s| s == "-d") =>
            {
                tracing::debug!("qemu args contains a `-d` flag, skipping capturing");
                false
            }
            _ => true,
        }
    }

    #[tracing::instrument(skip(self), level = "debug")]
    fn spawn_qemu(&self, qemu: &mut Command, binary: &Path) -> Result<Child> {
        let (Cmd::Run { qemu_settings, .. } | Cmd::Test { qemu_settings, .. }) = self;

        if self.should_capture() {
            qemu.stdout(Stdio::piped()).stderr(Stdio::piped());
        }

        // if we're running gdb, ensure that qemu doesn't exit, and block qemu
        // from inheriting stdin, so it's free for gdb to use.
        if qemu_settings.gdb {
            qemu.stdin(Stdio::piped()).arg("-no-shutdown");
        }

        let mut child = qemu.spawn().context("spawning qemu failed")?;

        // If the `--gdb` flag was passed, try to run gdb & connect to the kernel.
        if qemu_settings.gdb {
            crate::gdb::run_gdb(binary, qemu_settings.gdb_port)?;
            if let Err(error) = child.kill() {
                tracing::error!(?error, "failed to kill qemu");
            }
        }

        Ok(child)
    }

    #[tracing::instrument(skip(self, paths), level = "debug")]
    pub fn run_qemu(&self, image: &Path, paths: &crate::Paths, uefi: bool) -> Result<()> {
        let mut qemu = Command::new("qemu-system-x86_64");
        qemu.arg("-drive")
            .arg(format!("format=raw,file={}", image.display()))
            .arg("-no-reboot");
        if uefi {
            qemu.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
        }

        match self {
            Cmd::Run {
                serial,
                qemu_settings,
            } => {
                tracing::info!(
                    "running mycelium in QEMU ({})",
                    paths.relative(image).display()
                );
                if *serial {
                    tracing::debug!("configured QEMU to output serial on stdio");
                    qemu.arg("-serial").arg("stdio");
                }

                qemu_settings.configure(&mut qemu);
                qemu.arg("--no-shutdown");

                let mut child = self.spawn_qemu(&mut qemu, paths.kernel_bin())?;
                if child.stdout.is_some() {
                    tracing::debug!("should capture qemu output");
                    let out = child.wait_with_output()?;
                    if out.status.success() {
                        return Ok(());
                    }

                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let status = out.status.code();
                    Err(format_err!("qemu exited with a non-zero status code"))
                        .with_section(move || format!("{status:?}").header("status code:"))
                        .with_section(move || stdout.trim().to_string().header("stdout:"))
                        .with_section(move || stderr.trim().to_string().header("stderr:"))
                } else {
                    tracing::debug!("not capturing qemu output");
                    let status = child.wait()?;
                    if status.success() {
                        return Ok(());
                    }
                    let status = status.code();
                    Err(format_err!("qemu exited with a non-zero status code"))
                        .with_section(move || format!("{status:?}").header("status code:"))
                }
            }

            Cmd::Test {
                qemu_settings,
                timeout_secs,
                ..
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
                tracing::info!("running kernel tests ({})", paths.relative(image).display());
                qemu_settings.configure(&mut qemu);
                qemu.args(TEST_ARGS);

                let mut child = self.spawn_qemu(&mut qemu, paths.kernel_bin())?;
                let stdout = child
                    .stdout
                    .take()
                    .map(|stdout| std::thread::spawn(move || TestResults::watch_tests(stdout)));

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
                                Ok(())
                            } else {
                                Err(format_err!("QEMU exited with status code {}", code))
                            }
                        } else {
                            Err(format_err!("QEMU exited without a status code, wtf?"))
                        }
                    }
                }
                .context("tests failed");
                tracing::debug!("tests done");

                if let Some(res) = stdout {
                    tracing::trace!("collecting stdout");
                    let res = res.join().unwrap()?;
                    eprintln!("{res}");
                    // exit with an error if the tests failed.
                    if !res.failed.is_empty() {
                        std::process::exit(1);
                    }

                    Ok(())
                } else {
                    tracing::warn!("no stdout from QEMU process?");
                    res
                }
            }
        }
    }
}

impl Settings {
    fn configure(&self, cmd: &mut Command) {
        // tell QEMU to be a generic 4-core x86_64 machine by default.
        //
        // this is so tests are run with the same machine regardless of whether
        // KVM or other accelerators are available, unless a specific QEMU
        // configuration is requested.
        const DEFAULT_QEMU_ARGS: &[&str] = &["-cpu", "qemu64", "-smp", "cores=4"];
        if self.gdb {
            tracing::debug!(gdb_port = self.gdb_port, "configuring QEMU to wait for GDB");
            cmd.arg("-S")
                .arg("-gdb")
                .arg(format!("tcp::{}", self.gdb_port));
        }

        if !self.qemu_args.is_empty() {
            tracing::info!(qemu.args = ?self.qemu_args, "configuring qemu");
            cmd.args(&self.qemu_args[..]);
        } else {
            tracing::info!(qemu.args = ?DEFAULT_QEMU_ARGS, "using default qemu args");
            cmd.args(DEFAULT_QEMU_ARGS);
        }
    }
}

fn parse_secs(s: &str) -> Result<Duration> {
    s.parse::<u64>()
        .map(Duration::from_secs)
        .context("not a valid number of seconds")
}

impl TestResults {
    fn watch_tests(output: impl std::io::Read) -> Result<Self> {
        use std::io::{BufRead, BufReader};
        let mut results = Self {
            tests: 0,
            completed: 0,
            failed: BTreeMap::new(),
            total: 0,
            panicked: 0,
            faulted: 0,
        };
        let mut lines = BufReader::new(output).lines();
        let colors = ColorMode::default();
        let green = colors.if_color(owo_colors::style().green());
        let red = colors.if_color(owo_colors::style().red());

        while let Some(line) = lines.next() {
            tracing::trace!(message = ?line);
            let line = line?;

            if let Some(count) = line.strip_prefix(mycotest::report::TEST_COUNT) {
                results.total = count
                    .trim()
                    .parse::<usize>()
                    .with_context(|| format!("parse string: {:?}", count.trim()))?;
            }

            if let Some(test) = TestName::parse_start(&line) {
                let _span =
                    tracing::debug_span!("test", "{}::{}", test.module(), test.name()).entered();
                tracing::debug!(?test, "found a test");
                eprint!("test {test} ...");
                results.tests += 1;

                let mut curr_output = Vec::new();
                let mut curr_outcome = None;
                for line in &mut lines {
                    tracing::trace!(message = ?line);
                    let line = match line {
                        Err(err) => {
                            tracing::debug!(?err, "unexpected qemu error");
                            curr_output.push(err.to_string());
                            break;
                        }
                        Ok(line) => line,
                    };

                    match TestName::parse_outcome(&line) {
                        Ok(None) => {}
                        Ok(Some((completed_test, outcome))) => {
                            ensure!(
                                test == completed_test,
                                "an unexpected test completed (actual: {completed_test}, expected: {test}, outcome={outcome:?})",
                            );
                            tracing::trace!(?outcome);
                            curr_outcome = Some(outcome);
                            break;
                        }
                        Err(err) => {
                            tracing::error!(?line, ?err, "failed to parse test outcome!");
                            return Err(
                                format_err!("failed to parse test outcome").note(err.to_string())
                            )
                            .note(format!("line: {line:?}"));
                        }
                    }

                    curr_output.push(line);
                }

                match curr_outcome {
                    Some(Ok(())) => eprintln!(" {}", "ok".style(green)),
                    Some(Err(mycotest::report::Failure::Fail)) => {
                        eprintln!(" {}", "not ok!".style(red));
                        results.failed.insert(test.to_static(), curr_output);
                    }
                    Some(Err(mycotest::report::Failure::Panic)) => {
                        eprintln!(" {}", "panic!".style(red));
                        results.failed.insert(test.to_static(), curr_output);
                        results.panicked += 1;
                    }
                    Some(Err(mycotest::report::Failure::Fault)) => {
                        eprintln!(" {}", "FAULT".style(red));
                        results.failed.insert(test.to_static(), curr_output);
                        results.faulted += 1;
                    }
                    None => {
                        tracing::info!("qemu exited unexpectedly! wtf!");
                        curr_output.push("<AND THEN QEMU EXITS???>".to_string());
                        eprintln!(" {}", "exit!".style(red));
                        results.failed.insert(test.to_static(), curr_output);
                        break;
                    }
                };

                results.completed += 1;
            }
        }

        tracing::trace!("lines ended");

        Ok(results)
    }
}

impl fmt::Display for TestResults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num_failed = self.failed.len();
        if num_failed > 0 {
            writeln!(f, "\nfailures:")?;
            for (test, output) in &self.failed {
                writeln!(f, "\n---- {test} serial ----\n{}\n", &output[..].join("\n"))?;
            }
            writeln!(f, "\nfailures:\n")?;
            for test in self.failed.keys() {
                writeln!(f, "\t{test}")?;
            }
        }
        let colors = ColorMode::default();
        let res = if !self.failed.is_empty() {
            "FAILED".style(colors.if_color(owo_colors::style().red()))
        } else {
            "ok".style(colors.if_color(owo_colors::style().green()))
        };

        let num_missed = self.total - (self.completed + num_failed);
        let panicked_faulted = if self.panicked > 0 || self.faulted > 0 {
            format!(" ({} panicked, {} faulted)", self.panicked, self.faulted)
        } else {
            String::new()
        };
        writeln!(
            f,
            "\ntest result: {res}. {} passed{panicked_faulted}; {num_failed} failed; {num_missed} missed; {} total",
            self.completed - num_failed,
            self.total
        )?;

        if num_missed > 0 {
            writeln!(
                f,
                "\n{}: {num_missed} tests didn't get to run due to a panic/fault",
                "warning".style(colors.if_color(owo_colors::style().yellow().bold())),
            )?;
        }

        Ok(())
    }
}
