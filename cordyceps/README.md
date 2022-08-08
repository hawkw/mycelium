# cordyceps

🍄 the [Mycelium] intrusive data structures library.

[![crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![Documentation (HEAD)][docs-main-badge]][docs-main-url]
[![MIT licensed][mit-badge]][mit-url]
[![Test Status][tests-badge]][tests-url]
[![Sponsor @hawkw on GitHub Sponsors][sponsor-badge]][sponsor-url]

[crates-badge]: https://img.shields.io/crates/v/cordyceps.svg
[crates-url]: https://crates.io/crates/cordyceps
[docs-badge]: https://docs.rs/cordyceps/badge.svg
[docs-url]: https://docs.rs/cordyceps
[docs-main-badge]: https://img.shields.io/netlify/3ec00bb5-251a-4f83-ac7f-3799d95db0e6?label=docs%20%28main%20branch%29
[docs-main-url]: https://mycelium.elizas.website/cordyceps
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: ../LICENSE
[tests-badge]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml/badge.svg?branch=main
[tests-url]: https://github.com/hawkw/mycelium/actions/workflows/ci.yml
[sponsor-badge]: https://img.shields.io/badge/sponsor-%F0%9F%A4%8D-ff69b4
[sponsor-url]: https://github.com/sponsors/hawkw
[Mycelium]: https://mycelium.elizas.website

## what is it?

This library provides a collection of intrusive data structures originally
implemented for the [Mycelium] operating system. Currently, it provides an
[intrusive doubly-linked list][list] and an [intrusive, lock-free MPSC
queue][queue].

## intrusive data structures

[Intrusive data structures][intrusive] are node-based data structures where the
node data (pointers to other nodes and, potentially, any associated metadata)
are stored _within_ the values that are contained by the data structure, rather
than owning those values.

### when should i use intrusive data structures?

- Because node data is stored *inside* of the elements of a collection, no
  additional heap allocation is required for those nodes. This means that when
  an element is already heap allocated, it can be added to a collection without
  requiring an additional allocation.
- Similarly, when elements are at fixed memory locations (such as pages in a
  page allocator, or `static`s), they can be added to intrusive data structures
  without allocating *at all*. This makes intrusive data structures useful in
  code that cannot allocate &mdash; for example, we might use intrusive lists of
  memory regions to *implement* a heap allocator.
- Intrusive data structures may offer better performance than other linked or
  node-based data structures, since allocator overhead is avoided.

### when shouldn't i use intrusive data structures?

- Intrusive data structures require the elements stored in a collection to be
  _aware_ of the collection. If a `struct` is to be stored in an intrusive
  collection, it will need to store a `Links` struct for that structure as a
  field, and implement the [`Linked`] trait to allow the intrusive data structure
  to access its `Links`.
- A given instance of a [`Linked`] type may not be added to multiple intrusive
  data structures *of the same type*. This can sometimes be worked around with
  multiple wrapper types. An object *may* be a member of multiple intrusive data
  structures of different types.
- Using intrusive data structures requires `unsafe` code. The [`Linked`] trait
  is unsafe to implement, as it requires that types implementing [`Linked`]
  uphold additional invariants. In particular, members of intrusive collections
  *must* be pinned in memory; they may not move (or be dropped) while linked
  into an intrusive collection.

## about the name

In keeping with Mycelium's fungal naming theme, _Cordyceps_ is a genus of
ascomycete fungi that's (in)famous for its [intrusive behavior][cordyceps].

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/cordyceps/README.md

| Feature | Default | Explanation |
| :---    | :---    | :---        |
| `no-cache-pad` | `false` | Inhibits cache padding for the `CachePadded` struct used for many linked list pointers. When this feature is NOT enabled, the size will be determined based on target platform. |
| `alloc`        | `false`  | Enables [`liballoc`] dependency and features that depend on `liballoc`. |
| `std`          | `false`  | Enables [`libstd`] dependency and features that depend on the Rust standard library. Implies `alloc`. |

[Mycelium]: https://github.com/hawkw/mycelium
[intrusive]: https://www.boost.org/doc/libs/1_45_0/doc/html/intrusive/intrusive_vs_nontrusive.html
[cordyceps]: https://en.wikipedia.org/wiki/Cordyceps#Biology
[list]: https://docs.rs/cordyceps/latest/cordyceps/struct.List.html
[queue]: https://docs.rs/cordyceps/latest/cordyceps/mpsc_queue/struct.MpscQueue.html
[`Linked`]: https://docs.rs/cordyceps/latest/cordyceps/trait.Linked.html
[`liballoc`]: https://doc.rust-lang.org/alloc/
[`libstd`]: https://doc.rust-lang.org/std/