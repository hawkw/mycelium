# mycelium

someday this will do stuff

[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw

## what is it?

this repository contains Mycelium, an alleged "operating system", and a
collection of Rust libraries written as part of its development. eventually,
Mycelium will be a microkernel-ish operating system that runs on commodity
desktop hardware, executes [WebAssembly] modules in both kernel- and user-space,
and implements the [WebAssembly System Interface][wasi] (or _WASI_) as its syscall layer.

right now, though, it runs...[_one_ webassembly module][helloworld], which
prints "hello world" and immediately exits. so that's coming along great,
thanks for asking.

## crates

> "what if the real operating system was the libraries we wrote along the way?"

in the process of implementing Mycelium itself, we've written a whole bunch of
Rust libraries, which also live in this repository. some of these libraries may
be generally useful for other projects, and have been or will be published to
[crates.io].

the general-purpose crates in this repository include:

- [`cordyceps`]: a library of [intrusive data structures][intrusive], including
      linked lists and lock-free MPSC queues,
- [`maitake`]: an "async runtime construction kit" for using Rust's async/await
      in bare metal applications such as operating systems,
- [`mycelium-alloc`]: an implementation of a [buddy-block memory allocator][buddy],
- [`mycelium-bitfield`]: a bitfield library based on const eval and declarative
      macros,
- [`mycelium-util`]: a "standard library" of reusable components for bare-metal
      `no_std` Rust projects, such as `no_std` synchronization primitives.

in addition, beyond the kernel itself, this repository also contains the
following Mycelium components:

- [`hal-core`]: the core interface definitions of Mycelium's (admittedly
      somewhat half-baked) hardware abstraction layer,
- [`hal-x86_64`]: implementations of the Mycelium HAL for 64-bit x86 (amd64)
      CPUs,
- [`mycotest`]: testing utilities for Mycelium's in-kernel tests,
- [`inoculate`]: Mycelium's Horrible Build Tool™

## how do i run it?

so...how do i actually build and run Mycelium?

the easiest way to run the Mycelium kernel (using the [QEMU] emulator) is to use
[`inoculate`], Mycelium's Horrible Build Tool™. to launch Mycelium in [QEMU], run

```console
cargo run-x64
```

see [CONTRIBUTING.md] for details on development environment setup and tools.

[WebAssembly]: https://webassembly.org/
[wasi]: https://github.com/WebAssembly/WASI
[helloworld]: https://github.com/hawkw/mycelium/blob/main/src/helloworld.wast
[crates.io]: https://crates.io

[`cordyceps]: https://github.com/hawkw/mycelium/tree/main/cordyceps
[`maitake`]: https://github.com/hawkw/mycelium/tree/main/maitake
[`mycelium-alloc`]: https://github.com/hawkw/mycelium/tree/main/alloc
[`mycelium-bitfield`]: https://github.com/hawkw/mycelium/tree/main/bitfield
[`mycelium-util`]: https://github.com/hawkw/mycelium/tree/main/util
[`hal-core`]: https://github.com/hawkw/mycelium/tree/main/hal-core
[`hal-x86_64`]: https://github.com/hawkw/mycelium/tree/main/hal-x86_64
[`mycotest`]: https://github.com/hawkw/mycelium/tree/main/mycotest
[`inoculate`]: https://github.com/hawkw/mycelium/tree/main/inoculate

[intrusive]:
    https://www.boost.org/doc/libs/1_45_0/doc/html/intrusive/intrusive_vs_nontrusive.html
[buddy]: https://en.wikipedia.org/wiki/Buddy_memory_allocation

[QEMU]: https://www.qemu.org/
[CONTRIBUTING.md]: https://github.com/hawkw/mycelium/blob/main/CONTRIBUTING.md