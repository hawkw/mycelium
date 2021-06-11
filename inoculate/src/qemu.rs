use crate::term::{ColorMode, OwoColorize};
use crate::{cargo_log, Result};
use color_eyre::{
    eyre::{ensure, format_err, WrapErr},
    Help, SectionExt,
};
use mycotest::{Outcome, Test};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fmt,
    path::Path,
    process::{Command, ExitStatus, Stdio},
    time::Duration,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub enum Cmd {
    /// Builds a bootable disk image and runs it in QEMU (implies: `build`).
    Run {
        /// Redirect the VM's serial output to stdout
        #[structopt(long, short)]
        serial: bool,

        /// Extra arguments passed to QEMU
        #[structopt(flatten)]
        qemu_settings: Settings,
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

        /// Show captured serial output of successful tests
        #[structopt(long)]
        show_output: bool,

        /// Extra arguments passed to QEMU
        #[structopt(flatten)]
        qemu_settings: Settings,
    },
}

#[derive(Debug, StructOpt)]
pub struct Settings {
    /// Listen for GDB connections.
    #[structopt(long, short)]
    gdb: bool,

    /// The TCP port to listen for debug connections on.
    #[structopt(long, default_value = "1234")]
    gdb_port: u16,

    /// Extra arguments passed to QEMU
    #[structopt(raw = true)]
    qemu_args: Vec<String>,
}

#[derive(Debug)]
struct TestResults {
    tests: usize,
    completed: usize,
    failed: BTreeMap<mycotest::Test<'static, String>, Vec<String>>,
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
            Cmd::Test { qemu_settings, .. } | Cmd::Run { qemu_settings, .. } => {
                qemu_settings.should_capture()
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn run_qemu(&self, image: &Path) -> Result<()> {
        // TODO(eliza): should we `which qemu` here?
        let mut qemu = Command::new("qemu-system-x86_64");
        qemu.arg("-drive")
            .arg(format!("format=raw,file={}", image.display()))
            .arg("--no-reboot");

        match self {
            Cmd::Run {
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
                if self.should_capture() {
                    tracing::debug!("should capture qemu output");
                    let out = qemu.output()?;
                    if out.status.success() {
                        return Ok(());
                    }

                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let status = out.status.code();
                    Err(format_err!("qemu exited with a non-zero status code"))
                        .with_section(move || format!("{:?}", status).header("status code:"))
                        .with_section(move || stdout.trim().to_string().header("stdout:"))
                        .with_section(move || stderr.trim().to_string().header("stderr:"))
                } else {
                    tracing::debug!("not capturing qemu output");
                    let status = qemu.status()?;
                    if status.success() {
                        return Ok(());
                    }
                    let status = status.code();
                    Err(format_err!("qemu exited with a non-zero status code"))
                        .with_section(move || format!("{:?}", status).header("status code:"))
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
                cargo_log!("Running", "kernel tests ({})", image.display());
                tracing::info!("running QEMU in test mode");
                qemu_settings.configure(&mut qemu);
                qemu.args(TEST_ARGS);

                let (mut child, stdout) = if self.should_capture() {
                    tracing::debug!("capturing QEMU stdout");
                    let mut child = qemu
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .context("spawning QEMU with captured stdout failed")?;
                    let stdout = child.stdout.take().expect("wtf");
                    eprintln!("");
                    let stdout = std::thread::spawn(move || TestResults::watch_tests(stdout));
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
                if let Some(res) = stdout {
                    tracing::trace!("collecting stdout");
                    let res = res.join().unwrap()?;
                    eprintln!("{}", res);

                    // exit with an error if the tests failed.
                    if !res.failed.is_empty() {
                        std::process::exit(1);
                    }
                    Ok(())
                    // res.with_section(move || stdout.trim().to_string().header("serial output:"))
                } else {
                    res
                }
            }
        }
    }
}

impl Settings {
    fn should_capture(&self) -> bool {
        if self.qemu_args.iter().map(String::as_str).any(|s| s == "-d") {
            tracing::debug!("qemu args contains a `-d` flag, skipping capturing");
            return false;
        }

        true
    }

    fn configure(&self, cmd: &mut Command) {
        if self.gdb {
            tracing::debug!(gdb_port = self.gdb_port, "configured QEMU to wait for GDB");
            cmd.arg("-s").arg("--gdb").arg(format!("tcp::{}", self.gdb));
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

impl TestResults {
    fn watch_tests(output: impl std::io::Read) -> Result<Self> {
        use std::io::{BufRead, BufReader};
        let mut results = Self {
            tests: 0,
            completed: 0,
            failed: BTreeMap::new(),
            total: 0,
        };
        let mut lines = BufReader::new(output).lines();
        let colors = ColorMode::default();
        let green = colors.if_color(owo_colors::style().green());
        let red = colors.if_color(owo_colors::style().red());

        'all_tests: while let Some(line) = lines.next() {
            let line = line?;
            tracing::trace!(message = %line);

            if let Some(count) = line.strip_prefix(mycotest::TEST_COUNT) {
                results.total = count
                    .trim()
                    .parse::<usize>()
                    .with_context(|| format!("parse string: {:?}", count.trim()))?;
            }

            if let Some(test) = Test::parse_start(&line) {
                tracing::debug!(?test, "found test");
                eprint!("test {} ...", test);
                results.tests += 1;

                let mut curr_output = Vec::new();
                for line in &mut lines {
                    let line = line?;
                    tracing::trace!(message = %line);

                    if let Some((completed_test, outcome)) = Test::parse_outcome(&line) {
                        ensure!(
                            test == completed_test,
                            "an unexpected test completed (actual: {}, expected: {}, outcome={:?})",
                            completed_test,
                            test,
                            outcome,
                        );

                        match outcome {
                            Outcome::Pass => eprintln!(" {}", "ok".style(green)),
                            Outcome::Fail => {
                                eprintln!(" {}", "not ok!".style(red));
                                results.failed.insert(test.to_static(), curr_output);
                            }
                        }

                        results.completed += 1;
                        continue 'all_tests;
                    }

                    curr_output.push(line);
                }
            }
        }

        Ok(results)
    }
}

impl fmt::Display for TestResults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num_failed = self.failed.len();
        if num_failed > 0 {
            writeln!(f, "\nfailures:")?;
            for (test, output) in &self.failed {
                writeln!(
                    f,
                    "\n---- {} serial ----\n{}\n",
                    test,
                    &output[..].join("\n")
                )?;
            }
            writeln!(f, "\nfailures:\n")?;
            for test in self.failed.keys() {
                writeln!(f, "\t{}", test,)?;
            }
        }
        let colors = ColorMode::default();
        let res = if !self.failed.is_empty() {
            "FAILED".style(colors.if_color(owo_colors::style().red()))
        } else {
            "ok".style(colors.if_color(owo_colors::style().green()))
        };

        let num_missed = self.total - (self.completed + num_failed);
        writeln!(
            f,
            "\ntest result: {}. {} passed; {} failed; {} missed; {} total",
            res,
            self.completed - num_failed,
            num_failed,
            num_missed,
            self.total
        )?;

        if num_missed > 0 {
            writeln!(
                f,
                "\n{}: {} tests didn't get to run due to a panic/fault",
                "note".style(colors.if_color(owo_colors::style().yellow().bold())),
                num_missed
            )?;
        }

        Ok(())
    }
}
