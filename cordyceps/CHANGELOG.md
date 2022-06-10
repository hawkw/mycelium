## cordyceps-v0.2.1 - (2022-06-09)

[e3fe8f8](https://github.com/hawkw/mycelium/e3fe8f84212fa5c4ac5865d36a3cad9267c98c7c)...[dc2e638](https://github.com/hawkw/mycelium/dc2e638e056e183ac6eedfa7b821393f5447ba45)


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
