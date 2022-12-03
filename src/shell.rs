//! A rudimentary kernel-mode command shell, primarily for debugging and testing
//! purposes.
//!
use crate::rt;
use mycelium_util::fmt::{self, Write};

pub struct Command<'cmd> {
    name: &'cmd str,
    help: &'cmd str,
    func: Option<fn(Context<'_>) -> Result<'_>>,
    usage: &'cmd str,
    subcommands: Option<&'cmd [Command<'cmd>]>,
}

#[derive(Debug)]
pub struct Error<'a> {
    line: &'a str,
    kind: ErrorKind<'a>,
}

pub type Result<'a> = core::result::Result<(), Error<'a>>;

#[derive(Debug)]
enum ErrorKind<'a> {
    UnknownCommand(&'a [Command<'a>]),

    SubcommandRequired(&'a [Command<'a>]),
    InvalidArguments { help: &'a str, arg: &'a str },
    Other(&'static str),
}

pub fn eval(line: &str) {
    static COMMANDS: &[Command] = &[DUMP, SLEEP, PANIC, FAULT];

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
    pub const fn new(name: &'cmd str) -> Self {
        Self {
            name,
            help: "",
            usage: "",
            func: None,
            subcommands: None,
        }
    }

    pub const fn with_help(self, help: &'cmd str) -> Self {
        Self { help, ..self }
    }

    pub const fn with_subcommands(self, subcommands: &'cmd [Self]) -> Self {
        Self {
            subcommands: Some(subcommands),
            ..self
        }
    }

    pub const fn with_fn(self, func: fn(Context<'_>) -> Result<'_>) -> Self {
        Self {
            func: Some(func),
            ..self
        }
    }

    pub const fn with_usage(self, usage: &'cmd str) -> Self {
        Self { usage, ..self }
    }

    pub fn run<'ctx>(&self, ctx: Context<'ctx>) -> Result<'ctx>
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
                    if let Some(func) = self.func {
                        func(ctx)
                    } else if current.is_empty() {
                        Err(ctx.subcommand_required(subcommands))
                    } else {
                        Err(e)
                    }
                }
                res => res,
            };
        }

        match self.func {
            Some(func) => func(ctx),
            None => Err(ctx.subcommand_required(self.subcommands.unwrap_or(&[]))),
        }
    }
}

impl fmt::Debug for Command<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            func,
            name,
            help,
            usage,
            subcommands,
        } = self;
        f.debug_struct("Command")
            .field("name", name)
            .field("help", help)
            .field("usage", usage)
            .field("func", &func.map(|func| fmt::ptr(func as *const ())))
            .field("subcommands", subcommands)
            .finish()
    }
}

impl fmt::Display for Command<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            func: _func,
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
        self.current
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

fn print_help(parent_cmd: &str, commands: &[Command]) {
    let parent_cmd_pad = if parent_cmd.is_empty() { "" } else { " " };
    for command in commands {
        tracing::info!(target: "shell", "  {parent_cmd}{parent_cmd_pad}{command}");
    }
    tracing::info!(target: "shell", "  {parent_cmd}{parent_cmd_pad}help --- prints this help message");
}
