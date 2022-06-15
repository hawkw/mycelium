# maitake

🎶🍄 ["Dancing mushroom"][maitake-wiki] &mdash; an async runtime construction
kit.

**✨ as seen in [the rust compiler test suite][97708]!**

[![crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[crates-badge]: https://img.shields.io/crates/v/maitake.svg
[crates-url]: https://crates.io/crates/maitake
[docs-badge]: https://docs.rs/maitake/badge.svg
[docs-url]: https://docs.rs/maitake
[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/maitake
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw
[maitake-wiki]: https://en.wikipedia.org/wiki/Grifola_frondosa
[97708]: https://github.com/rust-lang/rust/blob/c7b0452ece11bf714f7cf2003747231931504d59/src/test/ui/codegen/auxiliary/issue-97708-aux.rs

## what is it?

This library is a collection of modular components for building a Rust
async runtime based on [`core::task`] and [`core::future`], with a focus on
supporting `#![no_std]` projects.

Unlike other async runtime implementations, `maitake` does *not* provide a
complete, fully-functional runtime implementation. Instead, it provides reusable
implementations of common functionality, including a [task system],
[scheduling], and [notification primitives][wait]. These components may be
combined with other runtime services, such as timers and I/O resources, to
produce a complete, application-specific async runtime.

`maitake` was initially designed for use in the [mycelium] and [mnemOS]
operating systems, but may be useful for other projects as well.

[`core::task`]: https://doc.rust-lang.org/stable/core/task/index.html
[`core::future`]: https://doc.rust-lang.org/stable/core/future/index.html
[task system]: https://mycelium.elizas.website/maitake/task/index.html
[scheduling]: https://mycelium.elizas.website/maitake/scheduler/index.html
[wait]: https://mycelium.elizas.website/maitake/wait/index.html
[mycelium]: https://github.com/hawkw/mycelium
[mnemOS]: https://mnemos.jamesmunns.com

## usage considerations

`maitake` is intended primarily for use in bare-metal projects, such as
operating systems, operating system components, and embedded systems. These
bare-metal systems typically do not use the Rust standard library, so `maitake`
supports `#![no_std]` by default, and the use of [`liballoc`] is feature-flagged
for systems where [`liballoc`] is unavailable.

This intended use case has some important implications:

#### `maitake` is *not* a complete asynchronous runtime

This is in contrast to other async runtimes, like [`tokio`], [`async-std`], and
[`glommio`], which provide everything a userspace application needs to run
async tasks and perform asynchronous IO, with lower-level implementation
details encapsulated behind the runtime's API. In the bare-metal systems
`maitake` is intended for use in, however, it is often necessary to have more
direct control over lower-level implementation details of the runtime.

For example: in an asynchronous runtime, [tasks] must be stored in non-stack
memory. Runtimes like [`tokio`] and [`async-std`] use the standard library's
allocator and `Box` type to allocate tasks on the heap. In bare-metal systems,
though, [`liballoc`]'s heap allocator may not be available. Such a system may
have no ability to perform dynamic heap allocations, or may implement its own
allocator which may not be compatible with `liballoc`.

`maitake` is designed to still be usable in those cases &mdash; even a system
which cannot dynamically allocate memory could use `maitake` in order to
create and schedule a fixed set of tasks that are stored in `'static`s, essentially
allocating tasks at compile-time. Therefore, `maitake` provides [an
interface][Storage] for overriding the memory container in which tasks are
stored. In order to provide such an interface, however, `maitake` must expose
[the in-memory representation of spawned tasks][Task], which other runtimes
typically do not make part of their public APIs.

[`tokio`]: https://crates.io/crate/tokio
[`async-std`]: https://crates.io/crate/async-std
[`glommio`]: https://crates.io/crate/glommio
[tasks]: https://mycelium.elizas.website/maitake/task/index.html
[storage]: https://mycelium.elizas.website/maitake/task/index.html
[Task]: https://mycelium.elizas.website/maitake/task/struct.task
[Storage]: https://mycelium.elizas.website/maitake/task/trait.storage

#### `maitake` does not support unwinding

Rust supports multiple modes of handling [panics]: `panic="abort"` and
`panic="unwind"`. When a program is compiled with `panic="unwind"`, panics are
handled by [unwinding] the stack of the panicking thread. This allows the use of
APIs like [`catch_unwind`], which allows panics to be handled without
terminating the entire program. On the other hand, compiling with
`panic="abort"` means that all panics immediately terminate the program.

Bare-metal systems typically do not use stack unwinding. For programs which use
the Rust standard library, support for unwinding is provided by `std`. However,
in bare-metal, `#![no_std]` systems, it is necessary for the system to implement
its own unwinding system. Therefore, *`maitake` does not support unwinding*.

This is important to note, because supporting unwinding imposes additional
[safety considerations][UnwindSafe]. In order to safely support unwinding, many
parts of `maitake`, such as runtime internals and synchronization primitives,
would have to take extra steps to ensure that they cannot be left in an invalid
state during unwinding. Ensuring unwind-safety would require the use of standard
library APIs that are not available without `std`, so `maitake` does not ensure
unwind-safety.

This means that `maitake` **should not be used** in programs compiled with
`panic="unwind"`. Typically, no bare-metal program will fall into this category,
but if you are using `maitake` in a project which uses `std`, it is necessary to
explicitly disable unwinding in that project's `Cargo.toml`.

[panics]: https://doc.rust-lang.org/stable/std/macro.panic.html
[unwinding]: https://doc.rust-lang.org/nomicon/unwinding.html
[`catch_unwind`]: https://doc.rust-lang.org/std/panic/fn.catch_unwind.html
[UnwindSafe]: https://doc.rust-lang.org/stable/std/panic/trait.UnwindSafe.html

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/maitake/README.md

| Feature | Default | Explanation |
| :---    | :---    | :---        |
| `alloc` | `true`  | Enables [`liballoc`] dependency |
| `no-cache-pad` | `false` | Inhibits cache padding for the [`CachePadded`] struct. When this feature is NOT enabled, the size will be determined based on target platform. |

[`liballoc`]: https://doc.rust-lang.org/alloc/
[`CachePadded`]: https://mycelium.elizas.website/mycelium_util/sync/struct.cachepadded