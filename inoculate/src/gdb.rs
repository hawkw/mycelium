use crate::{cargo_log, Result};
use color_eyre::eyre::WrapErr;
use std::{
    path::Path,
    process::{Command, ExitStatus},
};

#[tracing::instrument]
pub fn run_gdb(binary: &Path, gdb_port: u16) -> Result<ExitStatus> {
    let gdb_path = wheres_gdb().context("failed to find gdb executable")?;
    cargo_log!("Found", "{}", gdb_path);
    let mut gdb = Command::new(gdb_path);

    // Set the file, and connect to the given remote, then advance to `kernel_main`.
    //
    // The `-ex` command provides a gdb command to which is run by gdb
    // before handing control over to the user, and we can use it to
    // configure the gdb session to connect to the gdb remote.
    gdb.arg("-ex").arg(format!("file {}", binary.display()));
    gdb.arg("-ex").arg(format!("target remote :{}", gdb_port));

    // Set a temporary breakpoint on `kernel_main` and continue to it to
    // skip the non-mycelium boot process.
    // XXX: Add a flag to skip doing this to allow debugging the boot process.
    gdb.arg("-ex").arg("tbreak mycelium_kernel::kernel_main");
    gdb.arg("-ex").arg("continue");

    cargo_log!("Running", "{:?}", gdb);
    // Try to run gdb.
    let status = gdb.status().context("failed to run gdb")?;

    tracing::debug!("gdb exited with status {:?}", status);
    Ok(status)
}

fn wheres_gdb() -> Result<String> {
    let output = Command::new("which")
        .arg("rust-gdb")
        .output()
        .context("running `which rust-gdb` failed")?;
    let mut which_gdb =
        String::from_utf8(output.stdout).context("`which rust-gdb` output was not a string")?;
    which_gdb.truncate(which_gdb.trim_end().len());
    if which_gdb.ends_with("not found") {
        return Ok(String::from("gdb"));
    }

    Ok(which_gdb)
}
