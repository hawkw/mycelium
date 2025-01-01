# maitake-sync

🎶🍄 ["Dancing mushroom"][maitake-wiki] &mdash; asynchronous synchronization
primitives from [`maitake`]

[![crates.io][crates-badge]][crates-url]
[![Changelog][changelog-badge]][changelog-url]
[![Documentation][docs-badge]][docs-url]
[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[crates-badge]: https://img.shields.io/crates/v/maitake-sync.svg
[crates-url]: https://crates.io/crates/maitake-sync
[changelog-badge]:
    https://img.shields.io/crates/v/maitake-sync?label=changelog&color=blue
[changelog-url]: https://github.com/hawkw/mycelium/blob/main/maitake-sync/CHANGELOG.md
[docs-badge]: https://docs.rs/maitake-sync/badge.svg
[docs-url]: https://docs.rs/maitake-sync
[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/maitake-sync
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw
[maitake-wiki]: https://en.wikipedia.org/wiki/Grifola_frondosa

## what is it?

This library is a collection of synchronization primitives for asynchronous Rust
software based on [`core::task`] and [`core::future`], with a focus on
supporting `#![no_std]` projects. It was initially developed as part of
[`maitake`], an "async runtime construction kit" intended for use in the
[mycelium] and [mnemOS] operating systems, but it may be useful for other
projects as well.

To learn a bit of the backstory behind `maitake-sync`, see the [announcement
post](https://www.elizas.website/announcing-maitake-sync.html)!

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

[_Synchronization primitives_][primitives] are tools for implementing
synchronization between [tasks][`core::task`]: to control which tasks can run at
any given time, and in what order, and to coordinate tasks' access to shared
resources. Typically, this synchronization involves some form of _waiting_. In
asynchronous systems, synchronization primitives allow tasks to wait by yielding
to the runtime scheduler, so that other tasks may run while they are waiting.

## a tour of `maitake-sync`

The following synchronization primitives are provided:

- [`Mutex`]: a fairly queued, asynchronous [mutual exclusion lock], for
      protecting shared data
- [`RwLock`]: a fairly queued, asynchronous [readers-writer lock], which
      allows concurrent read access to shared data while ensuring write
      access is exclusive
- [`Semaphore`]: an asynchronous [counting semaphore], for limiting the
      number of tasks which may run concurrently
- [`WaitCell`], a cell that stores a *single* waiting task's [`Waker`], so
      that the task can be woken by another task,
- [`WaitQueue`], a queue of waiting tasks, which are woken in first-in,
      first-out order
- [`WaitMap`], a set of waiting tasks associated with keys, in which a task
      can be woken by its key

In addition, the [`util` module] contains a collection of general-purpose
utilities for implementing synchronization primitives, and the [`blocking`
module] contains implementations of *non-async* blocking synchronization
primitives.

[`core::task`]: https://doc.rust-lang.org/stable/core/task/index.html
[`core::future`]: https://doc.rust-lang.org/stable/core/future/index.html
[`maitake`]: https://mycelium.elizas.website/maitake
[mycelium]: https://github.com/hawkw/mycelium
[mnemOS]: https://mnemos.dev
[primitives]: https://wiki.osdev.org/Synchronization_Primitives
[mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
[readers-writer lock]: https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
[counting semaphore]: https://en.wikipedia.org/wiki/Semaphore_(programming)
[`Waker`]: core::task::Waker
[`Mutex`]: https://docs.rs/maitake-sync/latest/maitake_sync/struct.Mutex.html
[`RwLock`]: https://docs.rs/maitake-sync/latest/maitake_sync/struct.RwLock.html
[`Semaphore`]: https://docs.rs/maitake-sync/latest/maitake_sync/struct.Semaphore.html
[`WaitCell`]: https://docs.rs/maitake-sync/latest/maitake_sync/struct.WaitCell.html
[`WaitQueue`]:
    https://docs.rs/maitake-sync/latest/maitake_sync/struct.WaitQueue.html
[`WaitMap`]:
    https://docs.rs/maitake-sync/latest/maitake_sync/struct.WaitMap.html
[`util` module]:
    https://docs.rs/maitake-sync/latest/maitake_sync/util/index.html
[`blocking` module]:
    https://docs.rs/maitake-sync/latest/maitake_sync/blocking/index.html

## usage considerations

`maitake-sync` is intended primarily for use in bare-metal projects, such as
operating systems, operating system components, and embedded systems. These
bare-metal systems typically do not use the Rust standard library, so
`maitake-sync` supports `#![no_std]` by default, and the use of [`liballoc`] is
feature-flagged for systems where [`liballoc`] is unavailable.

### support for atomic operations

In general, `maitake-sync` is a platform-agnostic library. It does not interact
directly with the underlying hardware, or use platform-specific features.
However, one aspect of `maitake-sync`'s implementation may differ slightly
across different target architectures: `maitake-sync` relies on atomic
operations integers. Sometimes, atomic operations on integers of specific widths
are needed (e.g., [`AtomicU64`]), which may not be available on all architectures.

In order to work on architectures which lack atomic operations on 64-bit
integers, `maitake-sync` uses the [`portable-atomic`] crate by Taiki Endo. This
crate crate polyfills atomic operations on integers larger than the platform's
pointer width, when these are not supported in hardware.

In most cases, users of `maitake-sync` don't need to be aware of `maitake-sync`'s use of
`portable-atomic`. If compiling `maitake-sync` for a target architecture that has
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

[`AtomicU64`]: https://doc.rust-lang.org/stable/core/sync/atomic/struct.AtomicU64.html
[`portable-atomic`]: https://crates.io/crates/portable-atomic
[`RUSTFLAGS`]: https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags
[single-core]: https://docs.rs/portable-atomic/latest/portable_atomic/#optional-cfg
[interrupt-cfgs]: https://github.com/taiki-e/portable-atomic/blob/HEAD/src/imp/interrupt/README.md

### overriding blocking mutex implementations

In addition to async locks, `maitake-sync` also provides a [`blocking`] module,
which contains blocking [`blocking::Mutex`] and [`blocking::RwLock`] types. Many of
`maitake-sync`'s async synchronization primitives, including [`WaitQueue`],
[`Mutex`], [`RwLock`], and [`Semaphore`], internally use the [`blocking::Mutex`]
type for wait-list synchronization. By default, this type uses a
[`blocking::DefaultMutex`][`DefaultMutex`] as the underlying mutex
implementation, which attempts to provide the best generic mutex implementation
based on the currently enabled feature flags.

However, in some cases, it may be desirable to provide a custom mutex
implementation.  Therefore, `maitake-sync`'s [`blocking::Mutex`] type, and the
async synchronization primitives that depend on it, are generic over a `Lock`
type parameter which may be overridden using the [`RawMutex`] and
[`ScopedRawMutex`] traits from the [`mutex-traits`] crate, allowing alternative
blocking mutex implementations to be used with `maitake-sync`. Using the
[`mutex-traits`] adapters in the [`mutex`] crate, `maitake-sync`'s types may
also be used with raw mutex implementations that implement traits from the
[`lock_api`] and [`critical-section`] crates.

See [the documentation on overriding mutex implementations][overriding] for more
details.

[`blocking`]:
    https://docs.rs/maitake-sync/latest/maitake_sync/blocking/index.html
[`blocking::Mutex`]:
    https://docs.rs/maitake-sync/latest/maitake_sync/blocking/struct.Mutex.html
[`blocking::RwLock`]:
    https://docs.rs/maitake-sync/latest/maitake_sync/blocking/struct.RwLock.html
[`DefaultMutex`]:
    https://docs.rs/maitake-sync/latest/maitake_sync/blocking/struct.DefaultMutex.html
[spinlock]: https://en.wikipedia.org/wiki/Spinlock
[`RawMutex`]:
    https://docs.rs/mutex-traits/latest/mutex_traits/trait.RawMutex.html
[`ScopedRawMutex`]:
    https://docs.rs/mutex-traits/latest/mutex_traits/trait.ScopedRawMutex.html
[`mutex-traits`]: https://crates.io/crates/mutex-traits
[`lock_api`]: https://crates.io/crates/lock_api
[`critical-section`]: https://crates.io/crates/critical-section
[overriding]:
    https://docs.rs/maitake-sync/latest/maitake_sync/blocking/index.html#overriding-mutex-implementations

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/maitake-suync/README.md

| Feature        | Default | Explanation |
| :---           | :---    | :---        |
| `alloc`        | `true`  | Enables [`liballoc`] dependency |
| `std`          | `false`  | Enables the Rust standard library, disabling `#![no-std]`. When `std` is enabled, the [`DefaultMutex`] type will use [`std::sync::Mutex`]. This implies the `alloc` feature. |
| `critical-section` | `false` | Enables support for the [`critical-section`] crate. This includes a variant of the [`DefaultMutex`] type that uses a critical section, as well as the [`portable-atomic`] crate's `critical-section` feature (as [discussed above](#support-for-atomic-operations)) |
| `no-cache-pad` | `false` | Inhibits cache padding for the [`CachePadded`] struct. When this feature is NOT enabled, the size will be determined based on target platform. |
| `tracing`      | `false` | Enables support for [`tracing`] diagnostics. Requires `liballoc`.|
| `core-error`   | `false` | Enables implementations of the [`core::error::Error` trait][core-error] for `maitake-sync`'s error types. *Requires a nightly Rust toolchain*. |

[`liballoc`]: https://doc.rust-lang.org/alloc/
[`CachePadded`]: https://docs.rs/maitake-sync/latest/maitake_sync/util/struct.CachePadded.html
[`tracing`]: https://crates.io/crates/tracing
[core-error]: https://doc.rust-lang.org/stable/core/error/index.html
[`std::sync::Mutex`]:
    https://doc.rust-lang.org/stable/std/sync/struct.Mutex.html
