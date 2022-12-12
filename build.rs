use color_eyre::{
    eyre::{eyre, WrapErr},
    Result,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> color_eyre::Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let wasm_path = PathBuf::from("src/asdf.wast");
    build_wasm(&wasm_path, &out_dir)
        .with_context(|| format!("building WASM `{}` failed!", wasm_path.display()))?;

    println!("cargo:rerun-if-changed=x86_64-mycelium.json");
    Ok(())
}

fn build_wasm(wasm: impl AsRef<Path>, out_dir: impl AsRef<Path>) -> Result<()> {
    let wasm = wasm.as_ref();
    // Build our helloworld.wast into binary.
    let binary = wat::parse_file(wasm)?;
    let file_stem = wasm
        .file_stem()
        .ok_or_else(|| eyre!("wast file path has no file stem"))?;
    fs::write(
        out_dir.as_ref().join(file_stem).with_extension("wasm"),
        binary,
    )?;

    println!("cargo:rerun-if-changed={}", wasm.display());
    Ok(())
}
