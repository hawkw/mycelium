# Changelog

All notable changes to this project will be documented in this file.
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## maitake-sync-v0.2.2 - (2025-08-16)

[e896466](https://github.com/hawkw/mycelium/e89646637bd946d27e265e7954887845707ba975)...[0c04556](https://github.com/hawkw/mycelium/0c045561b69ba6faff638a4b5cc0d08374667718)


### Fixed

- No-cas maitake-sync ([#538](https://github.com/hawkw/mycelium/issues/538)) ([5dfdd75](https://github.com/hawkw/mycelium/5dfdd75448c8c931628834de707a174524957542))

## maitake-sync-v0.2.1 - (2025-02-06)

[dd00208](https://github.com/hawkw/mycelium/dd0020892564c77ee4c20ffbc2f7f5b046ad54c8)...[dd00208](https://github.com/hawkw/mycelium/dd0020892564c77ee4c20ffbc2f7f5b046ad54c8)


### Fixed

- For cs-mutex, enter cs before locking mutex ([#514](https://github.com/hawkw/mycelium/issues/514)) ([bf8d93b](https://github.com/hawkw/mycelium/bf8d93b1dace827005a37a149fe59aeb970ab4a6), closes [#513](https://github.com/hawkw/mycelium/issues/513))
- Add proper `ScopedRawMutex` bound to `wait_map::Wait` ([#515](https://github.com/hawkw/mycelium/issues/515)) ([509a8b7](https://github.com/hawkw/mycelium/509a8b723306de80e533e248e7042abb720bce44))
- Impl `Future` for `mutex::Lock` with generic `ScopedRawMutex` ([#517](https://github.com/hawkw/mycelium/issues/517)) ([12c5b21](https://github.com/hawkw/mycelium/12c5b2164a42ba1cf99443aacae72f00c9a12339))

## maitake-sync-v0.2.0 - (2025-01-30)

[e43cad5](https://github.com/hawkw/mycelium/e43cad5e425cadae393a425520fe3a9a8cea71e1)...[e43cad5](https://github.com/hawkw/mycelium/e43cad5e425cadae393a425520fe3a9a8cea71e1)

### <a id = "maitake-sync-v0.2.0-breaking"></a>Breaking Changes
- **`mutex-traits` integration ([#482](https://github.com/hawkw/mycelium/issues/482))** ([99da7e1](99da7e140b4646af1e44ae4560c260def8b9c0a3))<br />Renamed `spin::Mutex` and `spin::RwLock` to `blocking::Mutex` and

### Added

- Rename `EnqueueWait` to `Subscribe` ([#481](https://github.com/hawkw/mycelium/issues/481)) ([c499252](https://github.com/hawkw/mycelium/c4992526f87f5c38813e6671baf99089fa24d7f0))
- [**breaking**](#maitake-sync-v0.2.0-breaking) `mutex-traits` integration ([#482](https://github.com/hawkw/mycelium/issues/482)) ([99da7e1](https://github.com/hawkw/mycelium/99da7e140b4646af1e44ae4560c260def8b9c0a3))
- Add missing `Default` implementations ([#509](https://github.com/hawkw/mycelium/issues/509)) ([af69f72](https://github.com/hawkw/mycelium/af69f72d15e57078ba244a3e15e99a98a738840b))

### Deprecated

- S/default_features/default-features ([#502](https://github.com/hawkw/mycelium/issues/502)) ([fb4f514](https://github.com/hawkw/mycelium/fb4f51489e1cd04607f7a29a2f83a73e5077d28e))

### Documented

- Fix`RwLock` doctest imports ([e51eb8a](https://github.com/hawkw/mycelium/e51eb8aa98e7609490fa674f408db32fd51caa70))
- Link to changelogs in published crate READMEs ([#485](https://github.com/hawkw/mycelium/issues/485)) ([73ba776](https://github.com/hawkw/mycelium/73ba776ca0c651431a4af9a97f45a71ba524b335))

## maitake-sync-v0.1.2 - (2024-07-18)

[c67c62f](https://github.com/hawkw/mycelium/c67c62fd7c7e537833be6e0559f61f30ed40d0ca)...[c67c62f](https://github.com/hawkw/mycelium/c67c62fd7c7e537833be6e0559f61f30ed40d0ca)


### Added

- Add `wait_for` and `wait_for_value` to WaitCell and WaitQueue ([#479](https://github.com/hawkw/mycelium/issues/479)) ([6dc5a84](https://github.com/hawkw/mycelium/6dc5a8429ffc170c1f086f756237cef9d451c0f2))
- Add `is_closed` methods to `WaitCell`/`WaitMap`/`WaitQueue` ([#480](https://github.com/hawkw/mycelium/issues/480)) ([c67c62f](https://github.com/hawkw/mycelium/c67c62fd7c7e537833be6e0559f61f30ed40d0ca))

## maitake-sync-v0.1.1 - (2024-01-27)

[6919b82](https://github.com/hawkw/mycelium/6919b8233eb5394edf836fde1fcbedae6721ae6c)...[dba0827](https://github.com/hawkw/mycelium/dba0827aae2f18bad477e7d82af17cc018bfe0c2)


### Added

- Add `spin::RwLock` ([#472](https://github.com/hawkw/mycelium/issues/472)) ([d6199bf](https://github.com/hawkw/mycelium/d6199bf365191f12df742fe9bdf5009a6da66810), closes [#470](https://github.com/hawkw/mycelium/issues/470))
- Add `into_inner` to locks ([#473](https://github.com/hawkw/mycelium/issues/473)) ([2c431d0](https://github.com/hawkw/mycelium/2c431d057c39db29a43448d1860f2a1739331b5a))
- Add `get_mut` to locks ([#473](https://github.com/hawkw/mycelium/issues/473)) ([98b362a](https://github.com/hawkw/mycelium/98b362a40007d51a6fbcd7f9c09dd02eb0ced281))
- Impl `Default` for locks ([#473](https://github.com/hawkw/mycelium/issues/473)) ([6adf597](https://github.com/hawkw/mycelium/6adf5978a1c80b69f164272a3058c75b83dc50b6))
- Nicer `Debug` impls for spinlocks ([#473](https://github.com/hawkw/mycelium/issues/473)) ([b3615cd](https://github.com/hawkw/mycelium/b3615cdff2120ae58adf2eb4fd47ec2da9173f43))
- Add `spin::RwLock::{reader_count, has_writer}` ([#473](https://github.com/hawkw/mycelium/issues/473)) ([dba0827](https://github.com/hawkw/mycelium/dba0827aae2f18bad477e7d82af17cc018bfe0c2))

### Documented

- Link to announcement in readme ([3dcf582](https://github.com/hawkw/mycelium/3dcf582a141088866e3d24953c1ea5d4c47248fe))
- Fix docs lints ([#468](https://github.com/hawkw/mycelium/issues/468)) ([b904673](https://github.com/hawkw/mycelium/b90467361f8df44a81e01ce12d30dab76f04879b))

### Fixed

- Fix unconditional recursion in `PartialEq` ([#468](https://github.com/hawkw/mycelium/issues/468)) ([7a8c368](https://github.com/hawkw/mycelium/7a8c36895dad2e7f5d58a839090ad8ff821d4040))
- Add `#[must_use]` for locks ([#473](https://github.com/hawkw/mycelium/issues/473)) ([f5a9e18](https://github.com/hawkw/mycelium/f5a9e1880c07c40673aeeae532e708b062e44c23))

## maitake-sync-v0.1.0 - (2023-09-04)


### Added

- Introduce a separate `maitake-sync` crate ([#462](https://github.com/hawkw/mycelium/issues/462)) ([8d85472](https://github.com/hawkw/mycelium/8d854724043fd199b8231596e00077ee2b4b6832))

<!-- generated by git-cliff -->
