//! A rudimentary kernel-mode command shell, primarily for debugging and testing
//! purposes.
//!
use core::fmt::Write;

use mycelium_util::fmt;

pub struct Command {
    pub name: &'static str,
    pub help: &'static str,
    pub func: Option<fn(&str) -> Result<(), Error<'_>>>,
    pub subcommands: Option<&'static [Command]>,
}

#[derive(Debug)]
pub struct Error<'a> {
    line: &'a str,
    kind: ErrorKind<'a>,
}

#[derive(Debug)]
enum ErrorKind<'a> {
    UnknownCommand(&'a [Command]),

    SubcommandRequired {
        name: &'static str,
        subcommands: &'a [Command],
    },
    InvalidArguments(&'static str),
    Other(&'static str),
}

pub fn eval(line: &str) {
    static COMMANDS: &[Command] = &[Command::new("dump")
        .with_help("print formatted representations of a kernel structure")
        .with_subcommands(&[
            Command::new("bootinfo")
                .with_help("print the boot information structure")
                .with_fn(|line| Err(Error::other(line, "not yet implemented"))),
            Command::new("archinfo")
                .with_help("print the architecture information structure")
                .with_fn(|line| Err(Error::other(line, "not yet implemented"))),
            Command::new("timer")
                .with_help("print the timer wheel")
                .with_fn(|_| {
                    tracing::info!(timer = ?crate::rt::TIMER);
                    Ok(())
                }),
            crate::rt::DUMP_RT,
            crate::arch::shell::DUMP_ARCH,
            Command::new("heap")
                .with_help("print kernel heap statistics")
                .with_fn(|_| {
                    tracing::info!(heap = ?crate::ALLOC.state());
                    Ok(())
                }),
        ])];

    tracing::info!("executing shell command: {line:?}\n");

    if line == "help" {
        tracing::info!("available commands:");
        print_help(COMMANDS);
        return;
    }

    match handle_command(line, COMMANDS) {
        Ok(_) => {}
        Err(error) => tracing::warn!("could not execute {line:?}: {error}"),
    }
}

pub fn handle_command<'cmd>(line: &'cmd str, commands: &'cmd [Command]) -> Result<(), Error<'cmd>> {
    let line = line.trim();
    for cmd in commands {
        if let Some(line) = line.strip_prefix(cmd.name) {
            let line = line.trim();
            return cmd.run(line);
        }
    }

    Err(Error::unknown_command(line, commands))
}

// === impl Error ===

impl Command {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            help: "",
            func: None,
            subcommands: None,
        }
    }

    pub const fn with_help(self, help: &'static str) -> Self {
        Self { help, ..self }
    }

    pub const fn with_subcommands(self, subcommands: &'static [Command]) -> Self {
        Self {
            subcommands: Some(subcommands),
            ..self
        }
    }

    pub const fn with_fn(self, func: fn(&str) -> Result<(), Error<'_>>) -> Self {
        Self {
            func: Some(func),
            ..self
        }
    }

    pub fn run<'cmd>(&self, line: &'cmd str) -> Result<(), Error<'cmd>> {
        let line = line.trim();

        if line == "help" {
            tracing::info!("{}: {}", self.name, self.help);
            if let Some(subcommands) = self.subcommands {
                tracing::info!("subcommands:");
                print_help(subcommands);
            }
            return Ok(());
        }

        if let Some(subcommands) = self.subcommands {
            match (handle_command(line, subcommands), self.func) {
                (
                    Err(Error {
                        kind: ErrorKind::UnknownCommand(_),
                        ..
                    }),
                    Some(func),
                ) => return func(line),
                (result, _) => return result,
            }
        }
        match self.func {
            Some(func) => func(line),
            None => Err(Error::subcommand_required(
                line,
                self.name,
                self.subcommands.unwrap_or(&[]),
            )),
        }
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            func,
            name,
            help,
            subcommands,
        } = self;
        f.debug_struct("Command")
            .field("name", name)
            .field("help", help)
            .field("func", &func.map(|func| fmt::ptr(func as *const ())))
            .field("subcommands", subcommands)
            .finish()
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            func: _func,
            name,
            help,
            subcommands: _subcommands,
        } = self;
        write!(f, "{name}: {help}")
    }
}

// === impl Error ===

impl<'a> Error<'a> {
    fn unknown_command(line: &'a str, commands: &'a [Command]) -> Self {
        Self {
            line,
            kind: ErrorKind::UnknownCommand(commands),
        }
    }

    fn subcommand_required(line: &'a str, name: &'static str, subcommands: &'a [Command]) -> Self {
        Self {
            line,
            kind: ErrorKind::SubcommandRequired { name, subcommands },
        }
    }

    pub fn invalid_argument(line: &'a str, help: &'static str) -> Self {
        Self {
            line,
            kind: ErrorKind::InvalidArguments(help),
        }
    }

    pub fn other(line: &'a str, msg: &'static str) -> Self {
        Self {
            line,
            kind: ErrorKind::Other(msg),
        }
    }
}

impl fmt::Display for Error<'_> {
    fn fmt(&self, mut f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::UnknownCommand(commands) => {
                f.write_str("unknown command, expected one of: [")?;
                fmt::comma_delimited(&mut f, commands.iter().map(|Command { name, .. }| name))?;
                f.write_char(']')?;
            }
            ErrorKind::InvalidArguments(help) => {
                write!(f, "invalid arguments: {:?}, usage: {help}", self.line)?
            }
            ErrorKind::SubcommandRequired { name, subcommands } => {
                writeln!(
                    f,
                    "the '{name}' command requires one of the following subcommands: ["
                )?;
                fmt::comma_delimited(&mut f, subcommands.iter().map(|Command { name, .. }| name))?;
                f.write_char(']')?;
            }
            ErrorKind::Other(msg) => f.write_str(msg)?,
        }

        Ok(())
    }
}

fn print_help(commands: &[Command]) {
    for Command { name, help, .. } in commands {
        tracing::info!(" - {name}: {help}");
    }
}
