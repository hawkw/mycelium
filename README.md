# mycelium

someday this will do stuff

## building & running

to build mycelium for x86_64, you need the following:

- a nightly rust compiler
- the `cargo xbuild` and `bootimage` tools
- the `rust-src` and `llvm-tools-preview` Rustup components

you can install these tools with:

```shell
cargo install cargo-xbuild bootimage
rustup component add rust-src llvm-tools-preview
```

then, you can build mycelium with:

```shell
cargo xbuild --target=x86_64-mycelium.json
```

to create a bootable disk image, run:

```shell
cargo bootimage --target=x86_64-mycelium.json
```

this creates a bootable disk image in the `target/x86_64/debug` directory.

finally, you can run mycelium in [QEMU] with:

```shell
cargo xrun --target=x86_64-mycelium-kernel.json
```

of course [QEMU] needs to be installed for this.

[QEMU]: https://www.qemu.org/

### cargo aliases

to make life easier, i've also added some cargo aliases:

- `cargo dev-env` installs the required dev environment tools (currently
  `cargo-xbuild` and `bootimage`)
- `cargo run-x64` is an alias for `cargo xrun
  --target=x86_64-mycelium-kernel.json` (so you don't need to type all that)
