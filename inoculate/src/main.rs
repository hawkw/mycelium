use clap::Parser;
use inoculate::{Args, Result};

fn main() -> Result<()> {
    Args::parse().init_term()?.run()
}
