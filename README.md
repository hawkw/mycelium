# mycelium

someday this will do stuff

## building & running

to build mycelium for x86_64, you need the following:

- the `cargo xbuild` and `bootimage` tools
- a nightly rust compiler

you can install these tools with:

```shell
cargo install cargo-xbuild bootimage
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
