//! A rudimentary kernel-mode command shell, primarily for debugging and testing
//! purposes.
//!
use crate::rt;
use mycelium_util::fmt::{self, Write};

/// Defines a shell command, including its name, help text, and how the command
/// is executed.
#[derive(Debug)]
pub struct Command<'cmd> {
    name: &'cmd str,
    help: &'cmd str,
    run: Option<RunKind<'cmd>>,
    usage: &'cmd str,
    subcommands: Option<&'cmd [Command<'cmd>]>,
}

#[derive(Debug)]
pub struct Error<'a> {
    line: &'a str,
    kind: ErrorKind<'a>,
}

pub type Result<'a> = core::result::Result<(), Error<'a>>;

pub trait Run: Send + Sync {
    fn run<'ctx>(&'ctx self, ctx: Context<'ctx>) -> Result<'ctx>;
}

#[derive(Debug)]
enum ErrorKind<'a> {
    UnknownCommand(&'a [Command<'a>]),

    SubcommandRequired(&'a [Command<'a>]),
    InvalidArguments { help: &'a str, arg: &'a str },
    Other(&'static str),
}

enum RunKind<'a> {
    Fn(fn(Context<'_>) -> Result<'_>),
    Runnable(&'a dyn Run),
}

pub fn eval(line: &str) {
    static COMMANDS: &[Command] = &[DUMP, SLEEP, PANIC, FAULT, crate::drivers::pci::LSPCI_CMD];

    let _span = tracing::info_span!(target: "shell", "$", message = %line).entered();
    tracing::info!(target: "shell", "");

    if line == "help" {
        tracing::info!(target: "shell", "available commands:");
        print_help("", COMMANDS);
        tracing::info!(target: "shell", "");
        return;
    }

    match handle_command(Context::new(line), COMMANDS) {
        Ok(_) => {}
        Err(error) => tracing::warn!(target: "shell", "error: {error}"),
    }

    tracing::info!(target: "shell", "");
}

#[derive(Copy, Clone)]
pub struct Context<'cmd> {
    line: &'cmd str,
    current: &'cmd str,
}

pub fn handle_command<'cmd>(ctx: Context<'cmd>, commands: &'cmd [Command]) -> Result<'cmd> {
    let chunk = ctx.current.trim();
    for cmd in commands {
        if let Some(current) = chunk.strip_prefix(cmd.name) {
            let current = current.trim();
            return cmd.run(Context { current, ..ctx });
        }
    }

    Err(ctx.unknown_command(commands))
}

// === commands ===

const DUMP: Command = Command::new("dump")
    .with_help("print formatted representations of a kernel structure")
    .with_subcommands(&[
        Command::new("bootinfo")
            .with_help("print the boot information structure")
            .with_fn(|ctx| Err(ctx.other_error("not yet implemented"))),
        Command::new("archinfo")
            .with_help("print the architecture information structure")
            .with_fn(|ctx| Err(ctx.other_error("not yet implemented"))),
        Command::new("timer")
            .with_help("print the timer wheel")
            .with_fn(|_| {
                tracing::info!(target: "shell", timer = ?rt::TIMER);
                Ok(())
            }),
        rt::DUMP_RT,
        crate::arch::shell::DUMP_ARCH,
        Command::new("heap")
            .with_help("print kernel heap statistics")
            .with_fn(|_| {
                tracing::info!(target: "shell", heap = ?crate::ALLOC.state());
                Ok(())
            }),
    ]);

const SLEEP: Command = Command::new("sleep")
    .with_help("spawns a task to sleep for SECONDS")
    .with_usage("<SECONDS>")
    .with_fn(|ctx| {
        use maitake::time;

        let line = ctx.command().trim();
        if line.is_empty() {
            return Err(ctx.invalid_argument("expected a number of seconds to sleep for"));
        }

        let secs: u64 = line
            .parse()
            .map_err(|_| ctx.invalid_argument("number of seconds must be an integer"))?;
        let duration = time::Duration::from_secs(secs);

        tracing::info!(target: "shell", ?duration, "spawning a sleep");
        rt::spawn(async move {
            time::sleep(duration).await;
            tracing::info!(target: "shell", ?duration, "slept");
        });

        Ok(())
    });

const PANIC: Command = Command::new("panic")
    .with_usage("<MESSAGE>")
    .with_help("cause a kernel panic with the given message. use with caution.")
    .with_fn(|line| {
        panic!("{}", line.current);
    });

const FAULT: Command = Command::new("fault")
    .with_help("cause a CPU fault (divide-by-zero). use with caution.")
    .with_fn(|_| {
        unsafe {
            core::arch::asm!(
                "div {0:e}",
                in(reg) 0,
            )
        }
        Ok(())
    });

// === impl Command ===

impl<'cmd> Command<'cmd> {
    /// Constructs a new `Command` with the given `name`.
    ///
    /// By default, this command will have no help text, no subcommands, no
    /// usage hints, and do nothing. Use the [`Command::with_help`] and
    /// [`Command::with_usage`] to add help text to the command. Use
    /// [`Command::with_subcommands`] to add subcommands, and/or
    /// [`Command::with_fn`] or [`Command::with_runnable`] to add a function
    /// that defines how to execute the command.
    #[must_use]
    pub const fn new(name: &'cmd str) -> Self {
        Self {
            name,
            help: "",
            usage: "",
            run: None,
            subcommands: None,
        }
    }

    /// Add help text to the command.
    ///
    /// This should define what the command does, and is printed when running
    /// `help` commands.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mycelium_kernel::shell::Command;
    ///
    /// const DUMP: Command = Command::new("dump")
    ///     .with_help("print formatted representations of a kernel structure");
    ///
    /// // The shell will format this command's help text as:
    /// let help_text = "dump --- print formatted representations of a kernel structure";
    /// assert_eq!(format!("{DUMP}"), help_text);
    /// ```
    #[must_use]
    pub const fn with_help(self, help: &'cmd str) -> Self {
        Self { help, ..self }
    }

    /// Add usage text to the command.
    ///
    /// This should define what, if any, arguments the command takes. If the
    /// command does not take any arguments, it is not necessary to call this
    /// method.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mycelium_kernel::shell::Command;
    ///
    /// const ECHO: Command = Command::new("echo")
    ///     .with_help("print the provided text")
    ///     .with_usage("<TEXT>");
    ///
    /// // The shell will format this command's help text as:
    /// let help_text = "echo <TEXT> --- print the provided text";
    /// assert_eq!(format!("{ECHO}"), help_text);
    /// ```
    #[must_use]
    pub const fn with_usage(self, usage: &'cmd str) -> Self {
        Self { usage, ..self }
    }

    /// Add a list of subcommands to this command.
    ///
    /// If subcommands are added, they will be handled automatically by checking
    /// if the next argument matches the name of a subcommand, before calling
    /// the command's [function] or [runnable], if it has one.
    ///
    /// If the next argument matches the name of a subcommand, that subcommand
    /// will be automatically executed. If it does *not* match a subcommand, and
    /// the command has a function or runnable, that function or runnable will
    /// be called with the remainder of the input. If the command does not have
    /// a function or runnable, a "subcommand expected" error is returned.
    ///
    /// # Examples
    ///
    /// A command with only subcommands, and no root function/runnable:
    ///
    /// ```rust
    /// use mycelium_kernel::shell::Command;
    ///
    /// // let's pretend we're implementing git (inside the mycelium kernel? for
    /// // some reason????)...
    /// const GIT: Command = Command::new("git")
    ///     .with_subcommands(&[
    ///         SUBCMD_ADD,
    ///         SUBCMD_COMMIT,
    ///         SUBCMD_PUSH,
    ///         // more git commands ...
    ///     ]);
    ///
    /// const SUBCMD_ADD: Command = Command::new("add")
    ///     .with_help("add file contents to the index")
    ///     .with_fn(|ctx| {
    ///         // ...
    ///         # drop(ctx); Ok(())
    ///     });
    /// const SUBCMD_COMMIT: Command = Command::new("commit")
    ///     .with_help("record changes to the repository")
    ///     .with_fn(|ctx| {
    ///         // ...
    ///         # drop(ctx); Ok(())
    ///     });
    /// const SUBCMD_PUSH: Command = Command::new("push")
    ///     .with_help("update remote refs along with associated objects")
    ///     .with_fn(|ctx| {
    ///         // ...
    ///         # drop(ctx); Ok(())
    ///     });
    /// // more git commands ...
    /// # drop(GIT);
    /// ```
    ///
    /// [function]: Command::with_fn
    /// [runnable]: Command::with_runnable
    #[must_use]
    pub const fn with_subcommands(self, subcommands: &'cmd [Self]) -> Self {
        Self {
            subcommands: Some(subcommands),
            ..self
        }
    }

    /// Add a function that's run to execute this command.
    ///
    /// If [`Command::with_fn`] or [`Command::with_runnable`] was previously
    /// called, this overwrites the previously set value.
    #[must_use]
    pub const fn with_fn(self, func: fn(Context<'_>) -> Result<'_>) -> Self {
        Self {
            run: Some(RunKind::Fn(func)),
            ..self
        }
    }

    /// Add a [runnable item] that's run to execute this command.
    ///
    /// If [`Command::with_fn`] or [`Command::with_runnable`] was previously
    /// called, this overwrites the previously set value.
    ///
    /// [runnable item]: Run
    #[must_use]
    pub const fn with_runnable(self, run: &'cmd dyn Run) -> Self {
        Self {
            run: Some(RunKind::Runnable(run)),
            ..self
        }
    }

    /// Run this command in the provided [`Context`].
    pub fn run<'ctx>(&'cmd self, ctx: Context<'ctx>) -> Result<'ctx>
    where
        'cmd: 'ctx,
    {
        let current = ctx.current.trim();

        if current == "help" {
            let name = ctx.line.strip_suffix(" help").unwrap_or("<???BUG???>");
            if let Some(subcommands) = self.subcommands {
                tracing::info!(target: "shell", "{name} <COMMAND>: {help}", help = self.help);
                tracing::info!(target: "shell", "commands:");
                print_help(name, subcommands);
            } else {
                tracing::info!(target: "shell", "{name}");
            }
            return Ok(());
        }

        if let Some(subcommands) = self.subcommands {
            return match handle_command(ctx, subcommands) {
                Err(e) if e.is_unknown_command() => {
                    if let Some(ref run) = self.run {
                        run.run(ctx)
                    } else if current.is_empty() {
                        Err(ctx.subcommand_required(subcommands))
                    } else {
                        Err(e)
                    }
                }
                res => res,
            };
        }

        self.run
            .as_ref()
            .ok_or_else(|| ctx.subcommand_required(self.subcommands.unwrap_or(&[])))
            .and_then(|run| run.run(ctx))
    }
}

impl fmt::Display for Command<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            run: _func,
            name,
            help,
            usage,
            subcommands: _subcommands,
        } = self;

        let usage = if self.subcommands.is_some() && usage.is_empty() {
            "<COMMAND>"
        } else {
            usage
        };

        write!(
            f,
            "{name}{usage_pad}{usage} --- {help}",
            usage_pad = if !usage.is_empty() { " " } else { "" },
        )
    }
}

// === impl Error ===

impl Error<'_> {
    fn is_unknown_command(&self) -> bool {
        matches!(self.kind, ErrorKind::UnknownCommand(_))
    }
}

impl fmt::Display for Error<'_> {
    fn fmt(&self, mut f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn command_names<'cmd>(
            cmds: &'cmd [Command<'cmd>],
        ) -> impl Iterator<Item = &'cmd str> + 'cmd {
            cmds.iter()
                .map(|Command { name, .. }| *name)
                .chain(core::iter::once("help"))
        }

        let Self { line, kind } = self;
        match kind {
            ErrorKind::UnknownCommand(commands) => {
                write!(f, "unknown command {line:?}, expected one of: [")?;
                fmt::comma_delimited(&mut f, command_names(commands))?;
                f.write_char(']')?;
            }
            ErrorKind::InvalidArguments { arg, help } => {
                write!(f, "invalid argument {arg:?}: {help}")?
            }
            ErrorKind::SubcommandRequired(subcommands) => {
                writeln!(
                    f,
                    "the '{line}' command requires one of the following subcommands: ["
                )?;
                fmt::comma_delimited(&mut f, command_names(subcommands))?;
                f.write_char(']')?;
            }
            ErrorKind::Other(msg) => write!(f, "could not execute {line:?}: {msg}")?,
        }

        Ok(())
    }
}

// === impl Context ===

impl<'cmd> Context<'cmd> {
    pub const fn new(line: &'cmd str) -> Self {
        Self {
            line,
            current: line,
        }
    }

    pub fn command(&self) -> &'cmd str {
        self.current.trim()
    }

    fn unknown_command(&self, commands: &'cmd [Command]) -> Error<'cmd> {
        Error {
            line: self.line,
            kind: ErrorKind::UnknownCommand(commands),
        }
    }

    fn subcommand_required(&self, subcommands: &'cmd [Command]) -> Error<'cmd> {
        Error {
            line: self.line,
            kind: ErrorKind::SubcommandRequired(subcommands),
        }
    }

    pub fn invalid_argument(&self, help: &'static str) -> Error<'cmd> {
        Error {
            line: self.line,
            kind: ErrorKind::InvalidArguments {
                arg: self.current,
                help,
            },
        }
    }

    pub fn other_error(&self, msg: &'static str) -> Error<'cmd> {
        Error {
            line: self.line,
            kind: ErrorKind::Other(msg),
        }
    }
}

// === impl RunKind ===

impl RunKind<'_> {
    #[inline]
    fn run<'ctx>(&'ctx self, ctx: Context<'ctx>) -> Result<'ctx> {
        match self {
            Self::Fn(func) => func(ctx),
            Self::Runnable(runnable) => runnable.run(ctx),
        }
    }
}

impl fmt::Debug for RunKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fn(func) => f.debug_tuple("Run::Func").field(&fmt::ptr(func)).finish(),
            Self::Runnable(runnable) => f
                .debug_tuple("Run::Runnable")
                .field(&fmt::ptr(runnable))
                .finish(),
        }
    }
}

// === impl Run ===

impl<F> Run for F
where
    F: Fn(Context<'_>) -> Result<'_> + Send + Sync,
{
    fn run<'ctx>(&'ctx self, ctx: Context<'ctx>) -> Result<'ctx> {
        self(ctx)
    }
}

fn print_help(parent_cmd: &str, commands: &[Command]) {
    let parent_cmd_pad = if parent_cmd.is_empty() { "" } else { " " };
    for command in commands {
        tracing::info!(target: "shell", "  {parent_cmd}{parent_cmd_pad}{command}");
    }
    tracing::info!(target: "shell", "  {parent_cmd}{parent_cmd_pad}help --- prints this help message");
}
