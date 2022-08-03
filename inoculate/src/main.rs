use clap::Parser;
use inoculate::{Options, Result};

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut opts = Options::parse();
    opts.trace_init()?;
    opts.run()
}
