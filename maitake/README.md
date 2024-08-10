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
implementations of common functionality, including a [task system][task],
[scheduler], a [timer wheel][timer], and [synchronization primitives][sync].
These components may be combined with other runtime services, such as timers and
I/O resources, to produce a complete, application-specific async runtime.

`maitake` was initially designed for use in the [mycelium] and [mnemOS]
operating systems, but may be useful for other projects as well.

> **Note**
>
> This is a hobby project. I'm working on it in my spare time, for my own
> personal use. I'm very happy to share it with the broader Rust community, and
> [contributions] and [bug reports] are always welcome. However, please remember
> that I'm working on this library _for fun_, and if it stops being fun...well,
> you get the idea.
>
> Anyway, feel free to use and enjoy this crate, and to contribute back as much
> as you want to!

[contributions]: https://github.com/hawkw/mycelium/compare
[bug reports]: https://github.com/hawkw/mycelium/issues/new

[`core::task`]: https://doc.rust-lang.org/stable/core/task/index.html
[`core::future`]: https://doc.rust-lang.org/stable/core/future/index.html
[task]: https://mycelium.elizas.website/maitake/task/index.html
[scheduling]: https://mycelium.elizas.website/maitake/scheduler/index.html
[timer]: https://mycelium.elizas.website/maitake/time/struct.Timer.html
[sync]: https://mycelium.elizas.website/maitake/sync/index.html
[mycelium]: https://github.com/hawkw/mycelium
[mnemOS]: https://mnemos.dev

## a tour of `maitake`

`maitake` currently provides the following major API components:

- **[`maitake::task`][task]: the `maitake` task system**. This module contains the
  [`Task`] type, representing an asynchronous task (a [`Future`] that can be
  spawned on the runtime), and the [`TaskRef`] type, a reference-counted,
  type-erased pointer to a spawned [`Task`].

  Additionally, it also contains other utility types for working with tasks.
  These include the [`JoinHandle`] type, which can be used to await the output
  of a task once it has been spawned, and the [`task::Builder`] type, for
  configuring a task prior to spawning it.

- **[`maitake::scheduler`][scheduler]: schedulers for executing tasks**. In order to
  actually execute asynchronous tasks, one or more schedulers is required. This
  module contains the [`Scheduler`] and [`StaticScheduler`] types, which
  implement task schedulers, and utilities for constructing and using
  schedulers.

- **[`maitake::time`][time]: timers and futures for tracking time**. This module
  contains tools for waiting for time-based events in asynchronous systems. It
  provides the [`Sleep`] type, a [`Future`] which completes after a specified
  duration, and the [`Timeout`] type, which wraps another [`Future`] and cancels
  it if it runs for longer than a specified duration without completing.

  In order to use these futures, a system must have a timer. The `maitake::time`
  module therefore provides the [`Timer`] type, a hierarchical timer wheel which
  can track and notify a large number of time-based futures efficiently. A
  [`Timer`] must be [driven by a hardware time source][time-source], such as an
  interrupt or timestamp counter.

- **[`maitake::sync`][sync]: asynchronous synchronization primitives**. This
  module provides asynchronous implementations of common [synchronization
  primitives], including a [`Mutex`], [`RwLock`], and [`Semaphore`].
  Additionally, it provides lower-level synchronization types which may be
  useful when implementing custom synchronization strategies.

- **[`maitake::future`][future]: utility futures**. This module provides
  general-purpose utility [`Future`] types that may be used without the Rust
  standard library.

[`Task`]: https://mycelium.elizas.website/maitake/task/struct.Task.html
[`Future`]: https://doc.rust-lang.org/stable/core/future/trait.Future.html
[`TaskRef`]: https://mycelium.elizas.website/maitake/task/struct.TaskRef.html
[`JoinHandle`]: https://mycelium.elizas.website/maitake/task/struct.JoinHandle.html
[`task::Builder`]: https://mycelium.elizas.website/maitake/task/struct.Builder.html
[`Scheduler`]: https://mycelium.elizas.website/maitake/scheduler/struct.Scheduler.html
[`StaticScheduler`]: https://mycelium.elizas.website/maitake/scheduler/struct.StaticScheduler.html
[time]: https://mycelium.elizas.website/maitake/time/index.html
[`Sleep`]: https://mycelium.elizas.website/maitake/time/struct.Sleep.html
[`Timeout`]: https://mycelium.elizas.website/maitake/time/struct.Timeout.html
[`Timer`]: https://mycelium.elizas.website/maitake/time/struct.Timer.html
[time-source]: https://mycelium.elizas.website/maitake/time/timer/struct.Timer.html#driving-timers
[synchronization primitives]: https://wiki.osdev.org/Synchronization_Primitives
[`Mutex`]: https://mycelium.elizas.website/maitake/sync/struct.Mutex.html
[`RwLock`]: https://mycelium.elizas.website/maitake/sync/struct.RwLock.html
[`Semaphore`]: https://mycelium.elizas.website/maitake/sync/struct.Semaphore.html
[future]: https://mycelium.elizas.website/maitake/future/index.html

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

## platform support

In general, `maitake` is a platform-agnostic library. It does not interact
directly with the underlying hardware, or use platform-specific features (with
one small exception). Instead, `maitake` provides portable implementations of
core runtime components. In some cases, [such as the timer wheel][time-source],
downstream code must integrate `maitake`'s APIs with hardware-specific code for
in order to use them effectively.

### support for atomic operations

However, one aspect of `maitake`'s implementation may differ slightly across
different target architectures: `maitake` relies on atomic operations integers.
Sometimes, atomic operations on integers of specific widths are needed (e.g.,
[`AtomicU64`]), which may not be available on all architectures.

In order to work on architectures which lack atomic operations on 64-bit
integers, `maitake` uses the [`portable-atomic`] crate by Taiki Endo. This crate
crate polyfills atomic operations on integers larger than the platform's pointer
width, when these are not supported in hardware.

In most cases, users of `maitake` don't need to be aware of `maitake`'s use of
`portable-atomic`. If compiling `maitake` for a target architecture that has
native support for 64-bit atomic operations (such as `x86_64` or `aarch64`), the
native atomics are used automatically. Similarly, if compiling `maitake` for any
target that has atomic compare-and-swap operations on any size integer, but
lacks 64-bit atomics (i.e., 32-bit x86 targets like `i686`, or 32-bit ARM
targets with atomic operations), the `portable-atomic` polyfill is used
automatically. Finally, when compiling for target architectures which lack
atomic operations because they are *always* single-core, such as MSP430 or AVR
microcontrollers, `portable-atomic` simply uses unsynchronized operations with
interrupts temporarily disabled.

**The only case where the user must be aware of `portable-atomic` is when
compiling for targets which lack atomic operations but are not guaranteed to
always be single-core**. This includes ARMv6-M (`thumbv6m`), pre-v6 ARM (e.g.,
`thumbv4t`, `thumbv5te`), and RISC-V targets without the A extension. On these
architectures, the user must manually enable the [`RUSTFLAGS`] configuration
[`--cfg portable_atomic_unsafe_assume_single_core`][single-core] if (and **only
if**) the specific target hardware is known to be single-core. Enabling this cfg
is unsafe, as it will cause unsound behavior on multi-core systems using these
architectures.

Additional configurations for some single-core systems, which determine the
specific sets of interrupts that `portable-atomic` will disable when entering a
critical section, are described [here][interrupt-cfgs].

[time-source]: https://mycelium.elizas.website/maitake/time/struct.timer#driving-timers
[`AtomicU64`]: https://doc.rust-lang.org/stable/core/sync/atomic/struct.AtomicU64.html
[`portable-atomic`]: https://crates.io/crates/portable-atomic
[`RUSTFLAGS`]: https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags
[single-core]: https://docs.rs/portable-atomic/latest/portable_atomic/#optional-cfg
[interrupt-cfgs]:
    https://github.com/taiki-e/portable-atomic/blob/HEAD/src/imp/interrupt/README.md

### overriding blocking mutex implementations

In addition to async locks, [`maitake::sync`][sync] also provides a [`blocking`]
module, which contains blocking [`blocking::Mutex`] and [`blocking::RwLock`]
types. Many of `maitake::sync`'s async synchronization primitives, including
[`WaitQueue`], [`Mutex`], [`RwLock`], and [`Semaphore`], internally use the
[`blocking::Mutex`] type for wait-list synchronization. By default, this type
uses a [`blocking::DefaultMutex`][`DefaultMutex`] as the underlying mutex
implementation, which attempts to provide the best generic mutex implementation
based on the currently enabled feature flags.

However, in some cases, it may be desirable to provide a custom mutex
implementation. Therefore, `maitake::sync`'s [`blocking::Mutex`] type, and the
async synchronization primitives that depend on it, are generic over a `Lock`
type parameter which may be overridden using the [`RawMutex`] and
[`ScopedRawMutex`] traits from the [`mutex-traits`] crate, allowing alternative
blocking mutex implementations to be used with `maitake::sync`. Using the
[`mutex-traits`] adapters in the [`mutex`] crate, `maitake::sync`'s types may
also be used with raw mutex implementations that implement traits from the
[`lock_api`] and [`critical-section`] crates.

See [the documentation on overriding mutex implementations][overriding] for more
details.

[`blocking`]:
    https://mycelium.elizas.website/maitake/sync/blocking/index.html
[`blocking::Mutex`]:
    https://mycelium.elizas.website/maitake/sync/blocking/struct.Mutex.html
[`blocking::RwLock`]:
    https://mycelium.elizas.website/maitake/sync/blocking/struct.RwLock.html
[`DefaultMutex`]:
    https://mycelium.elizas.website/maitake/sync/blocking/struct.DefaultMutex.html
[`WaitQueue`]:
    https://mycelium.elizas.website/maitake/sync/struct.WaitQueue.html
[`WaitMap`]:
    https://mycelium.elizas.website/maitake/sync/struct.WaitMap.html
[spinlock]: https://en.wikipedia.org/wiki/Spinlock
[`RawMutex`]:
    https://docs.rs/mutex-traits/latest/mutex_traits/trait.RawMutex.html
[`ScopedRawMutex`]:
    https://docs.rs/mutex-traits/latest/mutex_traits/trait.ScopedRawMutex.html
[`mutex-traits`]: https://crates.io/crates/mutex-traits
[`lock_api`]: https://crates.io/crates/lock_api
[`critical-section`]: https://crates.io/crates/critical-section
[overriding]:
    https://mycelium.elizas.website/maitake/sync/blocking/index.html#overriding-mutex-implementations

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/maitake/README.md

| Feature        | Default | Explanation |
| :---           | :---    | :---        |
| `alloc`        | `true`  | Enables [`liballoc`] dependency |
| `std`          | `false`  | Enables the Rust standard library, disabling `#![no-std]`. When `std` is enabled, the [`DefaultMutex`] type will use [`std::sync::Mutex`]. This implies the `alloc` feature. |

| `critical-section` | `false` | Enables support for the [`critical-section`] crate. This includes a variant of the [`DefaultMutex`] type that uses a critical section, as well as the [`portable-atomic`] crate's `critical-section` feature (as [discussed above](#support-for-atomic-operations)) |
| `no-cache-pad` | `false` | Inhibits cache padding for the [`CachePadded`] struct. When this feature is NOT enabled, the size will be determined based on target platform. |
| `tracing-01`   | `false` | Enables support for v0.1.x of [`tracing`] (the current release version). Requires `liballoc`.|
| `tracing-02`   | `false` | Enables support for the upcoming v0.2 of [`tracing`] (via a Git dependency). |
| `core-error`   | `false` | Enables implementations of the [`core::error::Error` trait][core-error] for `maitake`'s error types. *Requires a nightly Rust toolchain*. |

[`liballoc`]: https://doc.rust-lang.org/alloc/
[`CachePadded`]: https://mycelium.elizas.website/mycelium_util/sync/struct.cachepadded
[`tracing`]: https://crates.io/crates/tracing
[core-error]: https://doc.rust-lang.org/stable/core/error/index.html
[`std::sync::Mutex`]:
    https://doc.rust-lang.org/stable/std/sync/struct.Mutex.html
