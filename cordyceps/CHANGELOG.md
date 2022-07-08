# Changelog

All notable changes to this project will be documented in this file.
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## cordyceps-v0.3.0 - (2022-06-25)

[f956111](https://github.com/hawkw/mycelium/f9561111fceead952261355594fa46e9027ca8dd)...[2a7ce9c](https://github.com/hawkw/mycelium/2a7ce9cc2fcda1808f327253092a5e8309aa882a)

### <a id = "cordyceps-v0.3.0-breaking"></a>Breaking Changes
- **Remove deprecated `Cursor` type alias ([#239](https://github.com/hawkw/mycelium/issues/239))** ([b4fcb16](b4fcb160214b2d44b5c740e4eb3c666fcd8dec3d))<br />This removes the `Cursor` type from `cordyceps::list`.
- **Remove deprecated `List::cursor` method ([#239](https://github.com/hawkw/mycelium/issues/239))** ([2e35a4b](2e35a4b82d5b5faa2ebfcefdf8a94885b32c3a99))<br />This removes the `List::cursor` method from `cordyceps::List`.
- **Pin `CursorMut` iterator `Item`s ([#240](https://github.com/hawkw/mycelium/issues/240))** ([5ee31ce](5ee31cee2312639800f27358e2ea1b41481d185e))<br />This changes the type signature of the `Iterator` impl for

### Added

- [**breaking**](#cordyceps-v0.3.0-breaking) Remove deprecated `Cursor` type alias ([#239](https://github.com/hawkw/mycelium/issues/239)) ([b4fcb16](https://github.com/hawkw/mycelium/b4fcb160214b2d44b5c740e4eb3c666fcd8dec3d))
- [**breaking**](#cordyceps-v0.3.0-breaking) Remove deprecated `List::cursor` method ([#239](https://github.com/hawkw/mycelium/issues/239)) ([2e35a4b](https://github.com/hawkw/mycelium/2e35a4b82d5b5faa2ebfcefdf8a94885b32c3a99))
- Add immutable `list::Cursor` type ([#241](https://github.com/hawkw/mycelium/issues/241)) ([5af5d48](https://github.com/hawkw/mycelium/5af5d488e431c004d7496237aac39fb0572eb137))
- Add `CursorMut::as_cursor` ([#244](https://github.com/hawkw/mycelium/issues/244)) ([2a7ce9c](https://github.com/hawkw/mycelium/2a7ce9cc2fcda1808f327253092a5e8309aa882a))

### Fixed

- [**breaking**](#cordyceps-v0.3.0-breaking) Pin `CursorMut` iterator `Item`s ([#240](https://github.com/hawkw/mycelium/issues/240)) ([5ee31ce](https://github.com/hawkw/mycelium/5ee31cee2312639800f27358e2ea1b41481d185e))

## cordyceps-v0.2.2 - (2022-06-21)

[7cdb821](https://github.com/hawkw/mycelium/7cdb82146fdddfa564d0ba78536da0b7579a63e0)...[f956111](https://github.com/hawkw/mycelium/f9561111fceead952261355594fa46e9027ca8dd)


### Added

- Add `Cursor::current` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([9edf815](https://github.com/hawkw/mycelium/9edf81534f68d59e656a9ea897c1aa058dcf5d61), closes [#224](https://github.com/hawkw/mycelium/issues/224))
- Add `Cursor::peek_next/peek_prev` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([7ae435b](https://github.com/hawkw/mycelium/7ae435bab55736e4282e10203ce97abec6fb8fa1), closes [#224](https://github.com/hawkw/mycelium/issues/224))
- Add `Cursor::move_next/move_prev` ([2c9e972](https://github.com/hawkw/mycelium/2c9e9720e8270716631b23eb99e06f993c064e95))
- Add `Cursor::remove_current` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([ed587ee](https://github.com/hawkw/mycelium/ed587eecd0e19e83d7233a8ba33120fe89e4b4e2), closes [#224](https://github.com/hawkw/mycelium/issues/224))
- Add `List::cursor_back_mut` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([b555204](https://github.com/hawkw/mycelium/b5552046a65ce017d751acde3cee54d95726cf4c), closes [#224](https://github.com/hawkw/mycelium/issues/224))
- Add `Cursor::index` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([f5652cd](https://github.com/hawkw/mycelium/f5652cdd02764321aea19bf12d2a8730604159ca), closes [#224](https://github.com/hawkw/mycelium/issues/224))
- Add `Cursor::insert_before/after` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([5d97b41](https://github.com/hawkw/mycelium/5d97b4193829d89d246bc20b2d50cb6daba331e0), closes [#224](https://github.com/hawkw/mycelium/issues/224))
- Add `iter::Extend` for `List` ([#232](https://github.com/hawkw/mycelium/issues/232)) ([1c59f93](https://github.com/hawkw/mycelium/1c59f93a95b0ab8a806f29e948b2a7de640b26cf), closes [#225](https://github.com/hawkw/mycelium/issues/225))
- Add `iter::FromIterator` for `List` ([#232](https://github.com/hawkw/mycelium/issues/232)) ([d9bec37](https://github.com/hawkw/mycelium/d9bec377c84e14068445523395d06b107c36d7dc), closes [#226](https://github.com/hawkw/mycelium/issues/226))
- Add `List::append` ([#233](https://github.com/hawkw/mycelium/issues/233)) ([0a0fd42](https://github.com/hawkw/mycelium/0a0fd420b0047008cc5bab0d6451054a4757ce20))
- Add `List::split_off`/`try_split_off` ([#233](https://github.com/hawkw/mycelium/issues/233)) ([48167ce](https://github.com/hawkw/mycelium/48167ce50d9b4174b4783ff32f226ff9386deb78))
- Add `Cursor::split_before`/`after` ([#233](https://github.com/hawkw/mycelium/issues/233)) ([1093c36](https://github.com/hawkw/mycelium/1093c36bc5bc9aaf46ac0041fb947431b8cad461))
- Add `Cursor::splice_before/after` ([#234](https://github.com/hawkw/mycelium/issues/234)) ([cd73585](https://github.com/hawkw/mycelium/cd735857da9e0e00bac3eb67a9e294c55df4f99c))

### Deprecated

- Rename `cursor` to `cursor_front_mut` ([#227](https://github.com/hawkw/mycelium/issues/227)) ([d41c0cd](https://github.com/hawkw/mycelium/d41c0cd98355eea687ca4d2b82e729f05546e096))
- Include deprecations in changelog ([#235](https://github.com/hawkw/mycelium/issues/235)) ([95d0ade](https://github.com/hawkw/mycelium/95d0ade3c3faed2d393d1e8d00495ad3284143d3))
- Rename `list::Cursor` to `CursorMut` ([#236](https://github.com/hawkw/mycelium/issues/236)) ([3035be4](https://github.com/hawkw/mycelium/3035be4fef6ca619c2800cd4c22ae39fbef7b4ee))

### Documented

- Improve `List` and `CursorMut` docs ([#237](https://github.com/hawkw/mycelium/issues/237)) ([7504b88](https://github.com/hawkw/mycelium/7504b88ecd97683f1e22132b2822aabcee487d1a))

### Fixed

- Missing `len` in `List` debug impl ([#233](https://github.com/hawkw/mycelium/issues/233)) ([dc926e3](https://github.com/hawkw/mycelium/dc926e39757c4e5e07b1900527541010a61c9881))
- Wrong `Cursor::split_before` behavior ([#234](https://github.com/hawkw/mycelium/issues/234)) ([5e3583c](https://github.com/hawkw/mycelium/5e3583c387ca31d7a0703908cbefe31c1b81293d))

## cordyceps-v0.2.1 - (2022-06-10)

[e3fe8f8](https://github.com/hawkw/mycelium/e3fe8f84212fa5c4ac5865d36a3cad9267c98c7c)...[7cdb821](https://github.com/hawkw/mycelium/7cdb82146fdddfa564d0ba78536da0b7579a63e0)


### Added

- `DoubleEndedIterator` for `List` ([#207](https://github.com/hawkw/mycelium/issues/207)) ([a9c4f1b](https://github.com/hawkw/mycelium/a9c4f1b0697a9fcda834d550ef6f2bc34dc14a02))
- Impl `ExactSizeIterator` for `List` ([#208](https://github.com/hawkw/mycelium/issues/208)) ([a5e6814](https://github.com/hawkw/mycelium/a5e681415d7a43f4facd5f9b89d9b36f220a3a71))
- Add `list::IterMut` ([#208](https://github.com/hawkw/mycelium/issues/208)) ([f5d6ea1](https://github.com/hawkw/mycelium/f5d6ea1e65ef4f10dc256555be0ceafba7639cb0))
- Impl `IntoIterator` for `List` ([#208](https://github.com/hawkw/mycelium/issues/208)) ([1e95127](https://github.com/hawkw/mycelium/1e9512700d9f4635eb5e704f48defb6e3cce448a))
- Add `List::{front, back, front_mut, back_mut}` ([#211](https://github.com/hawkw/mycelium/issues/211)) ([f120827](https://github.com/hawkw/mycelium/f12082763bb18b4622b8de95a31b23432b904d69))
- Add `List::drain_filter` ([#212](https://github.com/hawkw/mycelium/issues/212)) ([dc2e638](https://github.com/hawkw/mycelium/dc2e638e056e183ac6eedfa7b821393f5447ba45))

### Fixed

- Pin `list::IterMut` items ([#209](https://github.com/hawkw/mycelium/issues/209)) ([2e5a270](https://github.com/hawkw/mycelium/2e5a270235fc6a31efe61f61c128463b96ab02a2))

## cordyceps-v0.2.0 - (2022-06-07)

[cae707e](https://github.com/hawkw/mycelium/cae707ea55a5a755e4eafbbce2cee1fd8751e212)...[e3fe8f8](https://github.com/hawkw/mycelium/e3fe8f84212fa5c4ac5865d36a3cad9267c98c7c)

### <a id = "cordyceps-v0.2.0-breaking"></a>Breaking Changes
- **Fix use-after-free in `List` iterators ([#203](https://github.com/hawkw/mycelium/issues/203))** ([1eea1f2](1eea1f2290f0a858851a1fcb39d6d95c7b51cf37))<br />This changes the type signature of the `list::Iter` and `list::Cursor`
types.
- **Add `Drop` impl for `List` ([#203](https://github.com/hawkw/mycelium/issues/203))** ([ea7412a](ea7412ac2d7b31e98d8a69390db7a5b975569d90))<br />The `List::new` constructor now requires a `T: Linked<list::Links<T>>`
bound.

### Added

- Add `List::len` method ([#204](https://github.com/hawkw/mycelium/issues/204)) ([e286c61](https://github.com/hawkw/mycelium/e286c61f642dc9601f83edf2c33a1dd7d1637447))

### Fixed

- [**breaking**](#cordyceps-v0.2.0-breaking) Fix use-after-free in `List` iterators ([#203](https://github.com/hawkw/mycelium/issues/203)) ([1eea1f2](https://github.com/hawkw/mycelium/1eea1f2290f0a858851a1fcb39d6d95c7b51cf37))
- [**breaking**](#cordyceps-v0.2.0-breaking) Add `Drop` impl for `List` ([#203](https://github.com/hawkw/mycelium/issues/203)) ([ea7412a](https://github.com/hawkw/mycelium/ea7412ac2d7b31e98d8a69390db7a5b975569d90), fixes [#165](https://github.com/hawkw/mycelium/issues/165))

## cordyceps-v0.1.1 - (2022-06-06)

[8fe36c4](https://github.com/hawkw/mycelium/8fe36c49d724e77711e42717044832c45db3ed34)...[cae707e](https://github.com/hawkw/mycelium/cae707ea55a5a755e4eafbbce2cee1fd8751e212)


### Added

- Added new push_back and pop_front methods to list ([#198](https://github.com/hawkw/mycelium/issues/198)) ([c555772](https://github.com/hawkw/mycelium/c555772adf1ac6a58f0039a0ac9c8dea8b0bd38b), closes [#186](https://github.com/hawkw/mycelium/issues/186))

### Documented

- Add basic linked list examples ([#200](https://github.com/hawkw/mycelium/issues/200)) ([05c1509](https://github.com/hawkw/mycelium/05c15096db926675fb5453ecde711fa90b446849))
- Point README links at docs.rs ([#202](https://github.com/hawkw/mycelium/issues/202)) ([f0f2748](https://github.com/hawkw/mycelium/f0f27480793c2ce61d4057dbad3913de14830324))

## cordyceps-v0.1.0 - (2022-06-04)


### Added

- Add lock-free intrusive MPSC queue ([#136](https://github.com/hawkw/mycelium/issues/136)) ([b5d7d19](https://github.com/hawkw/mycelium/b5d7d191d86554bc1c04ddb229b29ffd6fc346ac))
- Initial working async executor ([#155](https://github.com/hawkw/mycelium/issues/155)) ([e1f5e12](https://github.com/hawkw/mycelium/e1f5e12d1f3f5a4bd40339e007649c223de692f7))
- Add cache padding inhibitor feature ([#161](https://github.com/hawkw/mycelium/issues/161)) ([bae38c7](https://github.com/hawkw/mycelium/bae38c78c506971c3d6d2d80fc2263e20f1965c3))
- Unsafe `MpscQueue` const constructor ([#163](https://github.com/hawkw/mycelium/issues/163)) ([7a3cede](https://github.com/hawkw/mycelium/7a3cede678be7467c79047b7f93bdbf5ff3f5d3a))

### Documented

- Fix broken links, add netlify config ([#144](https://github.com/hawkw/mycelium/issues/144)) ([d2dae58](https://github.com/hawkw/mycelium/d2dae5859cfafc903d10e7e4148ded381b1e88b4))
- Improve MPSC queue docs ([#160](https://github.com/hawkw/mycelium/issues/160)) ([3126dab](https://github.com/hawkw/mycelium/3126dabe4c3ddc52319007e153bfa325cd594be2))
- Fix typo: incosistent -> inconsistent ([#162](https://github.com/hawkw/mycelium/issues/162)) ([3ba91ae](https://github.com/hawkw/mycelium/3ba91aef42372986a1c1edde499cfef51980b4ad))
- Improve `maitake` & `cordyceps` documentation ([#195](https://github.com/hawkw/mycelium/issues/195)) ([2c05e9e](https://github.com/hawkw/mycelium/2c05e9ecc9aaa061ab86569587529aa17a92e23a))

### Fixed

- Add miri tests, fix stacked borrows ([#139](https://github.com/hawkw/mycelium/issues/139)) ([3a30fbd](https://github.com/hawkw/mycelium/3a30fbd59ff84db6d802849516d79f64f0b68371))
- Fix MPSC queue doctests ([#164](https://github.com/hawkw/mycelium/issues/164)) ([6b281fc](https://github.com/hawkw/mycelium/6b281fc31e2ffcf29b844d7020a30518378cee76))

<!-- generated by git-cliff -->
