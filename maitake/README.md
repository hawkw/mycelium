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

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/maitake/README.md

| Feature | Default | Explanation |
| :---    | :---    | :---        |
| `alloc` | `true`  | Enables [`liballoc`] dependency |
| `no-cache-pad` | `false` | Inhibits cache padding for the [`CachePadded`] struct. When this feature is NOT enabled, the size will be determined based on target platform. |

[`liballoc`]: https://doc.rust-lang.org/alloc/
[`CachePadded`]: https://mycelium.elizas.website/mycelium_util/sync/struct.cachepadded