# Changelog

All notable changes to this project will be documented in this file.
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## mycelium-bitfield-v0.1.5 - (2024-01-21)

[3063b08](https://github.com/hawkw/mycelium/3063b0807bbcdd7f1f7abab4df8d9709064fe1bc)...[7a8c368](https://github.com/hawkw/mycelium/7a8c36895dad2e7f5d58a839090ad8ff821d4040)


### Added

- Add `u128` support ([#467](https://github.com/hawkw/mycelium/issues/467)) ([54871d4](https://github.com/hawkw/mycelium/54871d4bffa7ba5b824c17db67685c186bc6c8e0), closes [#466](https://github.com/hawkw/mycelium/issues/466))

### Documented

- Hobby project disclaimer for published libs ([#463](https://github.com/hawkw/mycelium/issues/463)) ([230afdb](https://github.com/hawkw/mycelium/230afdbeae7cb4719c2bf177a6fe92a2ca4517ba))

### Fixed

- Missing lifetime in `enum_from_bits!` ([#468](https://github.com/hawkw/mycelium/issues/468)) ([ba41d68](https://github.com/hawkw/mycelium/ba41d68ca6255e34f92a46096bf5fafc8a83cc17))
- Fix `clippy::non_canonical_clone` warnings ([#468](https://github.com/hawkw/mycelium/issues/468)) ([f78ad1f](https://github.com/hawkw/mycelium/f78ad1f2ba161cdcacfd99a911f900e3c82a7ac9))
- Fix unconditional recursion in `PartialEq` ([#468](https://github.com/hawkw/mycelium/issues/468)) ([7a8c368](https://github.com/hawkw/mycelium/7a8c36895dad2e7f5d58a839090ad8ff821d4040))

## mycelium-bitfield-v0.1.3 - (2023-07-23)

[d0a6f13](https://github.com/hawkw/mycelium/d0a6f13cc53e0cd5dbd493b63ba0711fd06dc985)...[3063b08](https://github.com/hawkw/mycelium/3063b0807bbcdd7f1f7abab4df8d9709064fe1bc)


### Added

- Generate `fmt::UpperHex` and `LowerHex` ([#292](https://github.com/hawkw/mycelium/issues/292)) ([b6138c8](https://github.com/hawkw/mycelium/b6138c8b7b704c59e394fd5600c321d7eede0a46))
- Add `Pack::pair_with` ([#294](https://github.com/hawkw/mycelium/issues/294)) ([47b08b7](https://github.com/hawkw/mycelium/47b08b7506712cf5664cf62bf5f0c72fa226a994))
- Add `Pair::pack_from_{src, dst}` ([#294](https://github.com/hawkw/mycelium/issues/294)) ([48f48ca](https://github.com/hawkw/mycelium/48f48cab628c88edc4fa3ce6a045c522976a62e7))
- Use a separate IST stack for double faults ([#112](https://github.com/hawkw/mycelium/issues/112)) ([cff7467](https://github.com/hawkw/mycelium/cff74670029b2a797db4cb8664b5b513dd831b55), closes [#56](https://github.com/hawkw/mycelium/issues/56))
- Skip `_` fields in `fmt::Debug` ([#375](https://github.com/hawkw/mycelium/issues/375)) ([46ca526](https://github.com/hawkw/mycelium/46ca52615338f5341fd0006b56a20379a1a797de))
- Add `display_ascii` to generated bitfields ([#420](https://github.com/hawkw/mycelium/issues/420)) ([7842802](https://github.com/hawkw/mycelium/7842802e1b52da0d7939c972b69964cddc4f875a))
- Select Unicode format with `Display` alt-mode ([#420](https://github.com/hawkw/mycelium/issues/420)) ([a09d55b](https://github.com/hawkw/mycelium/a09d55b7dd81aed292ec8265ce28c8f70f0b293d))
- Add `enum_from_bits!` macro  ([#450](https://github.com/hawkw/mycelium/issues/450)) ([ab096b4](https://github.com/hawkw/mycelium/ab096b4bcf64beda092b3980ca32cb50ad4c2682), closes [#443](https://github.com/hawkw/mycelium/issues/443))

### Documented

- Fix main branch docs link ([#290](https://github.com/hawkw/mycelium/issues/290)) ([4dbdf37](https://github.com/hawkw/mycelium/4dbdf376aab37ba88d48abf6c25ec2f386f62c44))
- Replace tables with bulleted lists ([#451](https://github.com/hawkw/mycelium/issues/451)) ([10cd876](https://github.com/hawkw/mycelium/10cd8765964ce98e18787f7ba5c299ed7a11a86d))

### Fixed

- Remove recursion in `fmt::Binary` ([#292](https://github.com/hawkw/mycelium/issues/292)) ([0488696](https://github.com/hawkw/mycelium/04886961ba460fceb662fdff8f80d481e54ed241))
- Don't have `fmt::alt` control struct formatting ([#292](https://github.com/hawkw/mycelium/issues/292)) ([03055f0](https://github.com/hawkw/mycelium/03055f086dbdbb68be6cb2a6eef96ffa450df61c))
- Make `pair_with` work with other bitfields ([#295](https://github.com/hawkw/mycelium/issues/295)) ([532ee98](https://github.com/hawkw/mycelium/532ee987e0eddd203fc7ff3698a4c9f36232c669))
- Make `Packing` work with typed specs ([#295](https://github.com/hawkw/mycelium/issues/295)) ([7b86e81](https://github.com/hawkw/mycelium/7b86e811a77831f4745e2b9437b3125ac27be8c8))

## mycelium-bitfield-v0.1.2 - (2022-07-25)

[5d6d7d5](https://github.com/hawkw/mycelium/5d6d7d5f7fd5eb70b2ece8f9697b5d46ca908d6a)...[d0a6f13](https://github.com/hawkw/mycelium/d0a6f13cc53e0cd5dbd493b63ba0711fd06dc985)


### Fixed

- Respect 32 bit targets ([#255](https://github.com/hawkw/mycelium/issues/255)) ([9b02fae](https://github.com/hawkw/mycelium/9b02fae33166a07be80be9f619d1c2ad68186e84))
- Fix `raw_mask` shifting twice ([#262](https://github.com/hawkw/mycelium/issues/262)) ([8c34ef9](https://github.com/hawkw/mycelium/8c34ef9d43428afd963eee6472b88256927643b5))

## mycelium-bitfield-v0.1.1 - (2022-07-16)

[af8ad54](https://github.com/hawkw/mycelium/af8ad548baaa27ad6a5689ebf35164108ceeb181)...[5d6d7d5](https://github.com/hawkw/mycelium/5d6d7d5f7fd5eb70b2ece8f9697b5d46ca908d6a)


### Documented

- Fix broken links in README ([aaa6137](https://github.com/hawkw/mycelium/aaa6137dc58616e8ffabe918ac00d348852073a2))

## mycelium-bitfield-v0.1.0 - (2022-07-16)


### Added

- Bitfield macro thingy ([#168](https://github.com/hawkw/mycelium/issues/168)) ([e8a1e1a](https://github.com/hawkw/mycelium/e8a1e1a569404fa0e9dedcd9b5a231b4e0a2af17))
- Add `mycelium-bitfield` crate ([#171](https://github.com/hawkw/mycelium/issues/171)) ([ae9c79a](https://github.com/hawkw/mycelium/ae9c79a132b3b88ee8cc306f9a14031059d3fb87))
- Packing spec type safety ([#174](https://github.com/hawkw/mycelium/issues/174)) ([12ea600](https://github.com/hawkw/mycelium/12ea6004918a185b99af59a34a8a37f04d935e14))
- Add `..` to use all remaining bits ([#176](https://github.com/hawkw/mycelium/issues/176)) ([7855a55](https://github.com/hawkw/mycelium/7855a557932d8e498a1cebfe47a3d6d1882985fe))
- Add `WaitQueue` ([#191](https://github.com/hawkw/mycelium/issues/191)) ([85d5b00](https://github.com/hawkw/mycelium/85d5b00b9156de88777226325d0b1fb2e9ed596b))

### Documented

- Document generated methods ([#173](https://github.com/hawkw/mycelium/issues/173)) ([1ded218](https://github.com/hawkw/mycelium/1ded218e71800496433cc0b291e573fb529f8874))
- Add example generated code ([#172](https://github.com/hawkw/mycelium/issues/172)) ([5e5d0f4](https://github.com/hawkw/mycelium/5e5d0f4c834b4e1efd64e1c75689cbee70c1cb12))
- Add a README and lib.rs docs ([#254](https://github.com/hawkw/mycelium/issues/254)) ([777e379](https://github.com/hawkw/mycelium/777e379b55f12f2a4609392bffe738f009873820))
- Add missing docs for packing specs ([#254](https://github.com/hawkw/mycelium/issues/254)) ([f077270](https://github.com/hawkw/mycelium/f077270c63d8d6f443accaa8fdf737b284627e8f))
- Summarize generated code ([#254](https://github.com/hawkw/mycelium/issues/254)) ([5a053f6](https://github.com/hawkw/mycelium/5a053f62c194779798017aa70d0365d141a072f4))
- Use tables in generated method docs ([#254](https://github.com/hawkw/mycelium/issues/254)) ([b57dbe6](https://github.com/hawkw/mycelium/b57dbe660748d13fa134a56fc53badf9f9383143))
- Summarize the API in the README ([#254](https://github.com/hawkw/mycelium/issues/254)) ([c72450e](https://github.com/hawkw/mycelium/c72450e373baeee5ce1e4c03aafa24d492319ed8))
- Link back to mycelium ([#254](https://github.com/hawkw/mycelium/issues/254)) ([b8025e5](https://github.com/hawkw/mycelium/b8025e57943d5bacf098d41bdb2abd45fc39a1c8))

### Fixed

- `FromBits` for ints using all bits ([#176](https://github.com/hawkw/mycelium/issues/176)) ([5917629](https://github.com/hawkw/mycelium/591762938d4c329926e37ca99f58a48b89bcd44b))
- Macro generating broken doc links ([#176](https://github.com/hawkw/mycelium/issues/176)) ([1920795](https://github.com/hawkw/mycelium/192079584bbe4af57d6de81d73b1937cf6849e8b))

<!-- generated by git-cliff -->
