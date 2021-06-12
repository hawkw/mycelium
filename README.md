# mycelium

someday this will do stuff

## building & running

mycelium is built using [`inoculate`], the horrible mycelium build tool.

`inoculate` automates the process of compiling the kernel and bootloader and
producing a bootable mycelium disk image. it also provides a test runner for
running kernel-mode tests in QEMU, running the release kernel in QEMU, and
launching a [`gdb`] debugging session.

```
inoculate 0.1.0
the horrible mycelium build tool (because that's a thing we have to have now apparently!)

USAGE:
    inoculate [OPTIONS] <kernel-bin> [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --bootloader-manifest <bootloader-manifest>
            The path to the `bootloader` crate's Cargo manifest. If this is not provided, it will be located
            automatically
        --color <color>
            Whether to emit colors in output [env: CARGO_TERM_COLORS=]  [default: auto]  [possible values: auto, always,
            never]
        --kernel-manifest <kernel-manifest>
            The path to the kernel's Cargo manifest. If this is not provided, it will be located automatically

    -l, --log <log>                                    Configures build logging [env: RUST_LOG=]  [default: warn]
    -o, --out-dir <out-dir>                            Overrides the directory in which to build the output image
    -t, --target-dir <target-dir>                      Overrides the target directory for the kernel build

ARGS:
    <kernel-bin>    The path to the kernel binary

SUBCOMMANDS:
    help    Prints this message or the help of the given subcommand(s)
    run     Builds a bootable disk image and runs it in QEMU (implies: `build`)
    test    Builds a bootable disk image with tests enabled, and runs the tests in QEMU
```

[inoculate]: tree/main/inoculate

### cargo aliases

the following [cargo aliases] are provided for development:

- `cargo inoculate` runs any `inoculate` subcommand
- `cargo run-x64` runs the x86_64 kernel in QEMU
- `cargo test-x64` runs the x86_64 kernel's kernel-mode test suite in QEMU
- `cargo build-x64` builds the x86_64 bootable disk image
- `cargo clippy-x64` runs the [clippy] linter with the kernel x86_64 target
  enabled (rather than the build host's target triple).

[cargo aliases]: tree/main/.cargo/config
[clippy]: https://github.com/rust-lang/rust-clippy

### build dependencies

to build mycelium for x86_64, you need the following:

- a nightly rust compiler
- the `rust-src` and `llvm-tools-preview` Rust toolchain components

the `rust-src` and `llvm-tools-preview` toolchain components are required in the
[`rust-toolchain.toml`] file, so in most cases, rustup will install them
automatically. if, for whatever reason, they are not present, you can install
them manually with:

```shell
rustup component add rust-src llvm-tools-preview
```

[`rust-toolchain.toml`]: tree/main/rust-toolchain.toml

### runtime dependencies & tools

to run the mycelium kernel-mode tests, or to run an interactive mycelium session
using `inoculate`, you need the [QEMU] emulator; in particular,
`qemu-system-x86_64` to run the x86_64 mycelium kernel. you can install QEMU
using your favorite package manager.

similarly, to use `inoculate`'s support for [`gdb`] (the GNU debugger), you need
`gdb` installed on your system. it's probably also possible to debug mycelium with
`lldb` somehow, but i haven't tried that.

for NixOS users, there's an included [`shell.nix`] which can be used with
[`nix-shell`] or [`direnv`]. this should ensure all required runtime
dependencies and tools are present.

for users of other package managers, you're on your own...but if there's a
similar mechanism for installing the required dev tooling for a project from a
config file, i'd welcome a PR to add support for your preferred package manager.

[QEMU]: https://www.qemu.org/
[`gdb`]: https://www.gnu.org/software/gdb/
[`shell.nix`]: tree/main/rust-toolchain.toml
[`nix-shell`]: https://nixos.wiki/wiki/Development_environment_with_nix-shell
[`direnv`]: https://direnv.net/