# mycelium-util

🍄 a "standard library" for programming in the [Mycelium] kernel and related
libraries.

[![crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[crates-badge]: https://img.shields.io/crates/v/mycelium-util.svg
[crates-url]: https://crates.io/crates/mycelium-util
[docs-badge]: https://docs.rs/mycelium-util/badge.svg
[docs-url]: https://docs.rs/mycelium-util
[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/mycelium-util
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw

## overview

This crate is a collection of general-purpose utility types and traits for use
in the [Mycelium] operating system and its related libraries.

> **Warning**
> This crate is not *really* intended for general public consumption &mdash;
> it's written specifically for use in [Mycelium]. While it can be used
> elsewhere, it may have a number of Mycelium-specific quirks and design
> decisions. It's being published to crates.io primarily so that other crates
> which depend on it can be published, not because it's expected to be broadly
> useful outside of Mycelium.
>
> Also, because this crate exists primarily for use in Mycelium, breaking
> changes may be released frequently, and previous major versions will generally
> not be supported.

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/util/README.md

| Feature | Default | Explanation |
| :---    | :---    | :---        |
| `no-cache-pad` | `false` | Inhibits cache padding for the [`CachePadded`] struct. When this feature is NOT enabled, the size will be determined based on target platform. |
| `alloc`        | `false`  | Enables [`liballoc`] dependency |

[Mycelium]: https://mycelium.elizas.website
[`CachePadded`]: https://mycelium.elizas.website/mycelium_util/sync/struct.cachepadded
[`liballoc`]: https://doc.rust-lang.org/alloc/