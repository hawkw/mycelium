# mycelium-bitfield

🍄 bitfield utilities, courtesy of [Mycelium].

[![crates.io][crates-badge]][crates-url]
[![Changelog][changelog-badge]][changelog-url]
[![Documentation][docs-badge]][docs-url]
[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[crates-badge]: https://img.shields.io/crates/v/mycelium-bitfield.svg
[crates-url]: https://crates.io/crates/mycelium-bitfield
[changelog-badge]:
    https://img.shields.io/crates/v/mycelium-bitfield?label=changelog&color=blue
[changelog-url]: https://github.com/hawkw/mycelium/blob/main/mycelium-bitfield/CHANGELOG.md
[docs-badge]: https://docs.rs/mycelium-bitfield/badge.svg
[docs-url]: https://docs.rs/mycelium-bitfield
[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/mycelium_bitfield
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw

## what is it?

This library provides utilities for defining structured bitfields in Rust. It
consists of [a set of types][pack] for defining ranges that can be packed and
unpacked from an integer value, and a [`bitfield!` macro][`bitfield!`] for
generating bitfield types automatically using the packing types. These
components are modular: it's possible to use the packing spec types to
hand-write all of the code that the `bitfield!` macro would generate.

This crate was originally implemented for usage in the [Mycelium operating
system][Mycelium], although it is usable in other projects and does not depend
on any Mycelium-specific libraries.

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

## comparison with other crates

There are several other crates implementing bitfields or bitfield-related
utilities in Rust. These crates offer different, but sometimes overlapping,
functionality relative to `mycelium-bitfield`. In particular, the most directly
comparable crates that I'm currently aware of are the [`modular-bitfield`] and
[`bitflags`] libraries.

> **Note**
> This crate exists *primarily* because I thought it would be fun to write my
> own bitfield crate, not because the existing libraries were deficient. It
> *is* possible that I have a somewhat perverse conception of "fun"...
>
> The  [`modular-bitfield`] crate, in particular, can do most of the same things as
> `mycelium-bitfield`. However, there are some differences between the two
> libraries which may be interesting to consider.

* **[`bitflags`]**: The [`bitflags`] crate provides a declarative macro for
  generating a structured type representing a set of bitflags.

  The critical difference between [`bitflags`]' [`bitflags!`][bitflags-macro]
  macro and `mycelium-bitfield`'s [`bitfield!`] macro is that the [`bitflags`]
  crate only implements bit*flags*, not bit*fields*. It is not possible to
  define multi-bit structured ranges using [`bitflags`]; only single-bit flags
  can be set and unset.

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
  [`bitfield!` macro][`bitfield!`] is a declarative macro. In my opinion,
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

## usage

This crate's API consists of three primary components, the [packing spec
types](#packing-spec-types), the [`bitfield! macro`](#bitfield-macro), and the
[`FromBits` trait](#frombits-trait).

#### packing spec types

The [`pack` module][pack] defines a set of types that can be used to pack and
unpack ranges from integer values of various sizes, such as [`Pack64`] for
packing and unpacking a range from a `u64` value.

These packing spec types have `const fn` constructors that allow them to be
defined in relationship with each other. For example:

```rust
use mycelium_bitfield::Pack64;

// Defines a packing spec for the least-significant 12 bits of a 64-bit value.
const LOW: Pack64 = Pack64::least_significant(12);
// Defines a packing spec for the next 8 more-significant bits after `LOW`.
const MID: Pack64 = LOW.next(8);
// Defines a packing spec for the next 4 more-significant bits after `MID`.
const HIGH: Pack64 = MID.next(4);

// Wrap an integer value to pack it using method calls.
let coffee = Pack64::pack_in(0)
    // pack the 12 bits of `0xfee` at the range specified by `LOW`.
    .pack(0xfee, &LOW)
    // pack the 4 bits `0xc` at the range specified by `HIGH`.
    .pack(0xc, &HIGH)
    // pack `0xf` in the 8 bits specified by `MID`.
    .pack(0xf, &MID)
    // unwrap the packing value back into a `u64`.
    .bits();

assert_eq!(coffee, 0xc0ffee); // i want c0ffee
```

A majority of the functions in the [`pack`][pack] module are `const fn`s, allowing
the use of packing specs in const contexts.

See the [module-level docs for `pack`][pack] for details.

#### `bitfield!` macro

The [`bitfield!`][`bitfield!`] macro allows defining a structured bitfield type
declaratively. The macro will generate code that uses the `pack` module's
packing spec APIs to represent a bitfield type.

For example:
```rust
mycelium_bitfield::bitfield! {
    /// Bitfield types can have doc comments.
    #[derive(Eq, PartialEq)] // ...and attributes
    pub struct MyBitfield<u16> {
        /// Generates a packing spec named `HELLO` for the first 6
        /// least-significant bits.
        pub const HELLO = 6;

        // Fields with names starting with `_` can be used to mark bits as
        // reserved.
        const _RESERVED = 4;

        /// Generates a packing spec named `WORLD` for the next 3 bits.
        pub const WORLD = 3;

        /// A boolean value will generate a packing spec for a single bit.
        pub const FLAG: bool;
    }
}

// Bitfield types can be cheaply constructed from a raw numeric
// representation:
let bitfield = MyBitfield::from_bits(0b10100_0011_0101);

// `get` methods can be used to unpack fields from a bitfield type:
assert_eq!(bitfield.get(MyBitfield::HELLO), 0b11_0101);
assert_eq!(bitfield.get(MyBitfield::WORLD), 0b0101);

// `with` methods can be used to pack bits into a bitfield type by
// value:
let bitfield2 = MyBitfield::new()
    .with(MyBitfield::HELLO, 0b11_0101)
    .with(MyBitfield::WORLD, 0b0101);

assert_eq!(bitfield, bitfield2);

// `set` methods can be used to mutate a bitfield type in place:
let mut bitfield3 = MyBitfield::new();

bitfield3
    .set(MyBitfield::HELLO, 0b011_0101)
    .set(MyBitfield::WORLD, 0b0101);

assert_eq!(bitfield, bitfield3);
```

See the [`bitfield!`] macro's documentation for details on the macro's usage
and the code it generates.

#### `FromBits` trait

The [`FromBits`] trait can be implemented for user-defined types which can be
used as subfields of a [`bitfield!`]-generated structured bitfield type. This
trait may be manually implemented for any user-defined type that has a defined
bit representation, or generated automatically for `enum` types using the
[`enum_from_bits!`] macro.

For example:
```rust
use mycelium_bitfield::{bitfield, enum_from_bits, FromBits};

// An enum type can implement the `FromBits` trait if it has a
// `#[repr(uN)]` attribute.
#[repr(u8)]
#[derive(Debug, Eq, PartialEq)]
enum MyEnum {
    Foo = 0b00,
    Bar = 0b01,
    Baz = 0b10,
}

impl FromBits<u32> for MyEnum {
    // Two bits can represent all possible `MyEnum` values.
    const BITS: u32 = 2;
    type Error = &'static str;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        match bits as u8 {
            bits if bits == Self::Foo as u8 => Ok(Self::Foo),
            bits if bits == Self::Bar as u8 => Ok(Self::Bar),
            bits if bits == Self::Baz as u8 => Ok(Self::Baz),
            _ => Err("expected one of 0b00, 0b01, or 0b10"),
        }
    }

    fn into_bits(self) -> u32 {
        self as u8 as u32
    }
}

// Alternatively, the `enum_from_bits!` macro can be used to
// automatically generate a `FromBits` implementation for an
// enum type:
enum_from_bits! {
    #[derive(Debug, Eq, PartialEq)]
    pub enum MyGeneratedEnum<u8> {
        /// Isn't this cool?
        Wow = 0b1001,
        /// It sure is! :D
        Whoa = 0b0110,
    }
}

bitfield! {
    pub struct TypedBitfield<u32> {
        /// Use the first two bits to represent a typed `MyEnum` value.
        const ENUM_VALUE: MyEnum;

        /// Typed values and untyped raw bit fields can be used in the
        /// same bitfield type.
        pub const SOME_BITS = 6;

        /// The `FromBits` trait is also implemented for `bool`, which
        /// can be used to implement bitflags.
        pub const FLAG_1: bool;
        pub const FLAG_2: bool;

        /// `FromBits` is also implemented by (signed and unsigned) integer
        /// types. This will allow the next 8 bits to be treated as a `u8`.
        pub const A_BYTE: u8;

        /// We can also use the automatically generated enum:
        pub const OTHER_ENUM: MyGeneratedEnum;
    }
}

// Unpacking a typed value with `get` will return that value, or panic if
// the bit pattern is invalid:
let my_bitfield = TypedBitfield::from_bits(0b0010_0100_0011_0101_1001_1110);

assert_eq!(my_bitfield.get(TypedBitfield::ENUM_VALUE), MyEnum::Baz);
assert_eq!(my_bitfield.get(TypedBitfield::FLAG_1), true);
assert_eq!(my_bitfield.get(TypedBitfield::FLAG_2), false);
assert_eq!(my_bitfield.get(TypedBitfield::OTHER_ENUM), MyGeneratedEnum::Wow);

// The `try_get` method will return an error rather than panicking if an
// invalid bit pattern is encountered:

let invalid = TypedBitfield::from_bits(0b0011);

// There is no `MyEnum` variant for 0b11.
assert!(invalid.try_get(TypedBitfield::ENUM_VALUE).is_err());
```

See the [`FromBits` trait documentation][`FromBits`] for details on
implementing [`FromBits`] for user-defined types.

[Mycelium]: https://mycelium.elizas.website
[pack]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/pack/index.html
[`bitfield!`]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/macro.bitfield.html
[`modular-bitfield`]: https://crates.io/crates/modular-bitfield
[`bitflags`]: https://crates.io/crates/bitflags
[bitflags-macro]: https://docs.rs/bitflags/latest/bitflags/macro.bitflags.html
[`FromBits`]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/trait.FromBits.html
[`enum_from_bits!`]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/macro.enum_from_bits.html
[mbf-validation]:
    https://docs.rs/modular-bitfield/latest/modular_bitfield/#example-extra-safety-guard
[`Pack64`]:
    https://docs.rs/mycelium-bitfield/latest/mycelium_bitfield/pack/struct.Pack64.html
