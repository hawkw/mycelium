use crate::{term::ColorMode, Result};
use color_eyre::eyre::{ensure, WrapErr};
use std::{env, path::PathBuf, process::Command};

pub fn cmd(cmd: &str) -> Result<Command> {
    let cargo =
        wheres_cargo().with_context(|| format!("failed to build `cargo {}` command", cmd))?;
    let mut cargo = Command::new(cargo);
    cargo
        .arg(cmd)
        // propagate our color mode configuration
        .env("CARGO_TERM_COLOR", ColorMode::default().as_str());
    Ok(cargo)
}

pub fn wheres_cargo() -> Result<PathBuf> {
    if let Some(cargo) = env::var_os("CARGO") {
        let cargo = PathBuf::from(cargo);
        ensure!(
            cargo.exists(),
            "cargo path from CARGO env var ({}) does not exist",
            cargo.display()
        );
        return Ok(cargo);
    }

    let cargo = PathBuf::from(env!("CARGO"));
    ensure!(
        cargo.exists(),
        "cargo path from build-time CARGO env var ({}) does not exist",
        cargo.display()
    );
    Ok(cargo)
}
