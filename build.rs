use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // Build our helloworld.wast into binary.
    let binary = wat::parse_file("src/helloworld.wast")?;
    fs::write(out_dir.join("helloworld.wasm"), &binary)?;

    println!("cargo:rerun-if-changed=src/helloworld.wast");
    println!("cargo:rerun-if-changed=x86_64-mycelium.json");
    Ok(())
}
