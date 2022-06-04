# mycelium-util

a "standard library for programming in the [mycelium] kernel and related
libraries.

## features

The following features are available (this list is incomplete; you can help by [expanding it].)

[expanding it]: https://github.com/hawkw/mycelium/edit/main/util/README.md

| Feature | Default | Explanation |
| :---    | :---    | :---        |
| `no-cache-pad` | `false` | Inhibits cache padding for the [`CachePadded`] struct. When this feature is NOT enabled, the size will be determined based on target platform. |
| `alloc`        | `false`  | Enables [`liballoc`] dependency |

[mycelium]: https://mycelium.elizas.website
[`CachePadded`]: https://mycelium.elizas.website/mycelium_util/sync/struct.cachepadded
[`liballoc`]: https://doc.rust-lang.org/alloc/