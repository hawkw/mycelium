# mycelium-bitfield

🍄 bitfield utilities, courtesy of [Mycelium].

[![crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[crates-badge]: https://img.shields.io/crates/v/mycelium-bitfield.svg
[crates-url]: https://crates.io/crates/mycelium-bitfield
[docs-badge]: https://docs.rs/mycelium-bitfield/badge.svg
[docs-url]: https://docs.rs/mycelium-bitfield
[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/mycelium-bitfield
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw

## what is it?

This library provides utilities for defining structured bitfields in Rust. It
consists of [a set of types][pack] for defining ranges that can be packed and
unpacked from an integer value, and a [`bitfield!` macro][bitfield-macro] for
generating bitfield types automatically using the packing types. These
components are modular: it's possible to use the packing spec types to
hand-write all of the code that the `bitfield!` macro would generate.

## comparison with other crates

There are several other crates implementing bitfields or bitfield-related
utilities in Rust. These crates offer different, but sometimes overlapping,
functionality relative to `mycelium-bitfield`. In particular, the most directly
comparable crates that I'm currently aware of are the [`modular-bitfield`] and
[`bitflags`] libraries.

> **Note**
> This crate exists *primarily* because I thought it would be fun to write my
> own bitfield crate, not because the existing libraries were deficient. The
> [`modular-bitfield`] crate, in particular, can do most of the same things as
> `mycelium-bitfield`. However, there are some differences between the two
> libraries which may be interesting to consider.

* **[`bitflags`]**: The [`bitflags`] crate provides a declarative macro for
  generating a structured type representing a set of bitflags.

  The critical difference between [`bitflags`]' [`bitflags!`][bitflags-macro]
  macro and [`mycelium-bitfield`]'s [`bitfield!`][bitfield-macro] macro is that
  the [`bitflags`] crate only implements bit*flags*, not bit*fields*. It is not
  possible to define multi-bit structured ranges using [`bitflags`]; only single
  bit flags can be set and unset.

  However, the [`bitflags`] crate is widely used, has been around for a long
  time, and is relatively simple and lightweight. If all you need is a set of
  boolean, single-bit flags, it's a very solid choice. But, if you need
  `mycelium-bitfield`'s additional functionality for working with multi-bit
  ranges, note that it can also do most most of what the `bitflags` crate can
  do.

* **[`modular-bitfield`]**: The [`modular-bitfield`] crate provides a procedural
  macro for generating typed structured bitfields.

  The functionality implemented by [`modular-bitfield`] is broadly very similar
  to `mycelium-bitfield` &mdash; the two libraries can do most of the same
  things.

  The primary difference is that [`modular-bitfield`] is implemented
  using a procedural macro attribute, while `mycelium-bitfield`'s
  [`bitfield!` macro][bitfield-macro] is a declarative macro. In my opinion,
  this isn't a reason to prefer `mycelium-bitfield` over [`modular-bitfield`] in
  most use cases. I decided to try to write the whole thing using a declarative
  macro because I thought it would be a fun challenge, not because it's
  *better* (in fact, it would probably have been much easier to implement
  the bitfield type generation using a procedural macro). However, users who
  need to reduce or avoid procedural macros for some reason may want to consider
  choosing `mycelium-bitfield` for that reason.

  The other primary difference between `mycelium-bitfield` and
  [`modular-bitfield`] is that `mycelium-bitfield` also provides the
  [`pack`][pack] module with packing spec types. These types can be used to
  build bitfield types "by hand", in cases where different behavior from the
  macro-generated code is needed. [`modular-bitfield`] only provides a
  procedural macro, and does not have an equivalent to this lower-level
  interface.

  On the other hand, [`modular-bitfield`] provides
  [nicer validation][mbf-validation] that a typed value used as part of a
  bitfield actually fits in that bitfield. `mycelium-bitfield` cannot currently
  do this kind of compile-time checking, and relies on implementations of the
  [`FromBits` trait][`FromBits`] for user-provided types being correct.


[Mycelium]: https://mycelium.elizas.website
[pack]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/pack/index.html
[bitfield-macro]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/macro.bitfield.html
[`modular-bitfield`]: https://crates.io/crates/modular-bitfield
[`bitflags`]: https://crates.io/crates/bitflags
[`FromBits`]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/trait.FromBits.html
[mbf-validation]:
    https://docs.rs/modular-bitfield/latest/modular_bitfield/#example-extra-safety-guard