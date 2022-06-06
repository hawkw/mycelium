# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v0.1.1 (2022-06-06)

### Documentation

 - <csr-id-f0f27480793c2ce61d4057dbad3913de14830324/> point README links at docs.rs
   Now that we're on crates.io...
 - <csr-id-05c15096db926675fb5453ecde711fa90b446849/> add basic linked list examples
   * docs(cordyceps): add basic linked list examples
* misc docs fixy-uppy

### New Features

 - <csr-id-c555772adf1ac6a58f0039a0ac9c8dea8b0bd38b/> added new push_back and pop_front methods to list
   This PR adds some nice-to-have methods as detailed in [this issue](https://github.com/hawkw/mycelium/issues/186).

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 4 commits contributed to the release over the course of 1 calendar day.
 - 2 days passed between releases.
 - 3 commits where understood as [conventional](https://www.conventionalcommits.org).
 - 3 unique issues were worked on: [#198](https://github.comgit//hawkw/mycelium/issues/198), [#200](https://github.comgit//hawkw/mycelium/issues/200), [#202](https://github.comgit//hawkw/mycelium/issues/202)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#198](https://github.comgit//hawkw/mycelium/issues/198)**
    - added new push_back and pop_front methods to list ([`c555772`](https://github.comgit//hawkw/mycelium/commit/c555772adf1ac6a58f0039a0ac9c8dea8b0bd38b))
 * **[#200](https://github.comgit//hawkw/mycelium/issues/200)**
    - add basic linked list examples ([`05c1509`](https://github.comgit//hawkw/mycelium/commit/05c15096db926675fb5453ecde711fa90b446849))
 * **[#202](https://github.comgit//hawkw/mycelium/issues/202)**
    - point README links at docs.rs ([`f0f2748`](https://github.comgit//hawkw/mycelium/commit/f0f27480793c2ce61d4057dbad3913de14830324))
 * **Uncategorized**
    - Release cordyceps v0.1.1 ([`8164318`](https://github.comgit//hawkw/mycelium/commit/81643180d54f4cb77a6230bd120875388f8d9786))
</details>

## v0.1.0 (2022-06-04)

<csr-id-8fe36c49d724e77711e42717044832c45db3ed34/>
<csr-id-f0b1d63cdc0f6a21a696df231579a82cef930330/>
<csr-id-823dd80984909398d684743267f70b36ddc634ab/>
<csr-id-219fce179e495bc25d083df53a132fe1dfee8305/>
<csr-id-8b2ba7bdf5ec8182cc23ec424f9f84e0ffdf1fb1/>

### Chore

 - <csr-id-8fe36c49d724e77711e42717044832c45db3ed34/> add some more cargo metadata

 - <csr-id-f0b1d63cdc0f6a21a696df231579a82cef930330/> adopt MIT license globally
   apparently some people are actually interested in using bits of mycelium
   for other projects, which is kind of shocking to me. to make that
   possible, this branch adopts the MIT license for everything and adds
   `LICENSE` files to every crate, so that when they're published to
   crates.io, they'll be distributed along with a license.

### Documentation

<csr-id-3ba91aef42372986a1c1edde499cfef51980b4ad/>
<csr-id-3126dabe4c3ddc52319007e153bfa325cd594be2/>
<csr-id-d2dae5859cfafc903d10e7e4148ded381b1e88b4/>

 - <csr-id-2c05e9ecc9aaa061ab86569587529aa17a92e23a/> improve `maitake` & `cordyceps` documentation
   * docs(cordyceps): flesh out README
* docs(maitake): improve README
* docs(cordyceps): add API docs to everything
* 29aef9c docs(cordyceps): fix more broken links
* bf7d910 docs(cordyceps): fix link reference
* d8d4e01 docs(cordyceps): fix queue examples
* 0e37684 feat(cordyceps): add `OwnedConsumer::has_producers`
* 7af41d8 docs(cordyceps): improve MPSC queue docs
* fixup broken rustdoc
* add netlify config
* add README to kernel crate's docs

### New Features

 - <csr-id-7a3cede678be7467c79047b7f93bdbf5ff3f5d3a/> unsafe `MpscQueue` const constructor
   In order to permit constructing a `cordyceps::MpscQueue` in a `static`,
   this branch adds a new `const fn new_with_static_stub` to `MpscQueue`.
   This takes an `&'static T` as the stub node. Because this would
   potentially allow using the same stub node in multiple queues, this
   function is unfortunately unsafe.
   
   This branch also bumps the nightly version for `mycelium`, because
   `cordyceps` and `mycelium-util` now require Rust 1.61.0, for trait
   bounds in `const fn`s. The nightly bump required some changes to the
   Miri tests for cordyceps, as Miri now seems to consume more memory than
   it did previously.
 - <csr-id-bae38c78c506971c3d6d2d80fc2263e20f1965c3/> add cache padding inhibitor feature
   As requested!
   
   If this gets more complicated, it might be worth bringing in the 
   `cfg_if` crate.
 - <csr-id-e1f5e12d1f3f5a4bd40339e007649c223de692f7/> initial working async executor
   This branch adds a new `mycelium-async` crate with an initial
   implementation of task types and a single-core scheduler for async
   tasks. This is quite rough, but it works, so I'd really like to merge
   what I've got so far and move forward with cleaning it up in future PRs.
 - <csr-id-b5d7d191d86554bc1c04ddb229b29ffd6fc346ac/> add lock-free intrusive MPSC queue
   This branch adds an intrusive lock-free MPSC queue implementation. The
   queue is based on [Dmitry Vyukov's intrusive lock-free MPSC queue][1]
   from 1024cores.net. This is primarily intended for use in an upcoming
   async runtime for a queue of notified tasks for each runtime worker, but
   it may be useful for other things, as well (possibly in the allocator,
   though I'm not sure if it'll work there or not).
   
   The new lock-free MPSC queue is added in a new crate called `cordyceps`,
   which factors out the previous `mycelium_util::intrusive` module. This
   is so that the intrusive collections library can be published to
   crates.io, without making users depend on a bunch of mycelium-specific
   code.
   
   In order to implement this, I made some changes to the `list::Linked`
   trait so that it could be used for multiple types of intrusive data
   structure.

### Bug Fixes

 - <csr-id-6b281fc31e2ffcf29b844d7020a30518378cee76/> fix MPSC queue doctests
   These tests currently contain a `while` loop that only loops as long as
   producers exist. However, if the producer threads have terminated
   *before* consuming messages from the queue, some messages may be
   missed. This is --- naturally --- non-deterministic, and failures occur
   more frequently on CI.
   
   This branch fixes this by changing the test to "do-while"-style loops.
 - <csr-id-3a30fbd59ff84db6d802849516d79f64f0b68371/> add miri tests, fix stacked borrows
   this branch adds a configuration for running Miri tests. currently, this
   is primarily useful for testing `cordyceps` and friends, as the kernel
   and HAL code contains a bunch of bare-metal stuff Miri definitely won't
   understand.
   
   after running Miri tests, i've fixed a number of stacked borrows
   violations in the cordyceps linked list and MPSC queue modules. in
   particular, i've changed the `Linked` trait a bit --- `Linked::as_ptr`
   is now `Linked::into_ptr` and takes ownership of a `Handle`. this way,
   we'll never produce an owning `Handle` (such as a `Box`) from a raw
   pointer that originated from a shared reference (such as a
   `&Pin<Box<...>>`). shared refs can still be used when the _handle type
   itself_ is a shared ref. miri is now happy with us.

### Test

 - <csr-id-823dd80984909398d684743267f70b36ddc634ab/> decrease # messages in loom tests
   These take *way* too long to run on CI. This branch makes them way
   shorter.
 - <csr-id-219fce179e495bc25d083df53a132fe1dfee8305/> cordyceps always exposes loom cfg

 - <csr-id-8b2ba7bdf5ec8182cc23ec424f9f84e0ffdf1fb1/> bump loom to 0.5.5, rm custom loom tracing


### Commit Statistics

<csr-read-only-do-not-edit/>

 - 15 commits contributed to the release over the course of 146 calendar days.
 - 15 commits where understood as [conventional](https://www.conventionalcommits.org).
 - 13 unique issues were worked on: [#136](https://github.comgit//hawkw/mycelium/issues/136), [#139](https://github.comgit//hawkw/mycelium/issues/139), [#144](https://github.comgit//hawkw/mycelium/issues/144), [#155](https://github.comgit//hawkw/mycelium/issues/155), [#159](https://github.comgit//hawkw/mycelium/issues/159), [#160](https://github.comgit//hawkw/mycelium/issues/160), [#161](https://github.comgit//hawkw/mycelium/issues/161), [#162](https://github.comgit//hawkw/mycelium/issues/162), [#163](https://github.comgit//hawkw/mycelium/issues/163), [#164](https://github.comgit//hawkw/mycelium/issues/164), [#170](https://github.comgit//hawkw/mycelium/issues/170), [#195](https://github.comgit//hawkw/mycelium/issues/195), [#197](https://github.comgit//hawkw/mycelium/issues/197)

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **[#136](https://github.comgit//hawkw/mycelium/issues/136)**
    - add lock-free intrusive MPSC queue ([`b5d7d19`](https://github.comgit//hawkw/mycelium/commit/b5d7d191d86554bc1c04ddb229b29ffd6fc346ac))
 * **[#139](https://github.comgit//hawkw/mycelium/issues/139)**
    - add miri tests, fix stacked borrows ([`3a30fbd`](https://github.comgit//hawkw/mycelium/commit/3a30fbd59ff84db6d802849516d79f64f0b68371))
 * **[#144](https://github.comgit//hawkw/mycelium/issues/144)**
    - fix broken links, add netlify config ([`d2dae58`](https://github.comgit//hawkw/mycelium/commit/d2dae5859cfafc903d10e7e4148ded381b1e88b4))
 * **[#155](https://github.comgit//hawkw/mycelium/issues/155)**
    - initial working async executor ([`e1f5e12`](https://github.comgit//hawkw/mycelium/commit/e1f5e12d1f3f5a4bd40339e007649c223de692f7))
 * **[#159](https://github.comgit//hawkw/mycelium/issues/159)**
    - decrease # messages in loom tests ([`823dd80`](https://github.comgit//hawkw/mycelium/commit/823dd80984909398d684743267f70b36ddc634ab))
 * **[#160](https://github.comgit//hawkw/mycelium/issues/160)**
    - improve MPSC queue docs ([`3126dab`](https://github.comgit//hawkw/mycelium/commit/3126dabe4c3ddc52319007e153bfa325cd594be2))
 * **[#161](https://github.comgit//hawkw/mycelium/issues/161)**
    - add cache padding inhibitor feature ([`bae38c7`](https://github.comgit//hawkw/mycelium/commit/bae38c78c506971c3d6d2d80fc2263e20f1965c3))
 * **[#162](https://github.comgit//hawkw/mycelium/issues/162)**
    - fix typo: incosistent -> inconsistent ([`3ba91ae`](https://github.comgit//hawkw/mycelium/commit/3ba91aef42372986a1c1edde499cfef51980b4ad))
 * **[#163](https://github.comgit//hawkw/mycelium/issues/163)**
    - unsafe `MpscQueue` const constructor ([`7a3cede`](https://github.comgit//hawkw/mycelium/commit/7a3cede678be7467c79047b7f93bdbf5ff3f5d3a))
 * **[#164](https://github.comgit//hawkw/mycelium/issues/164)**
    - fix MPSC queue doctests ([`6b281fc`](https://github.comgit//hawkw/mycelium/commit/6b281fc31e2ffcf29b844d7020a30518378cee76))
 * **[#170](https://github.comgit//hawkw/mycelium/issues/170)**
    - adopt MIT license globally ([`f0b1d63`](https://github.comgit//hawkw/mycelium/commit/f0b1d63cdc0f6a21a696df231579a82cef930330))
 * **[#195](https://github.comgit//hawkw/mycelium/issues/195)**
    - improve `maitake` & `cordyceps` documentation ([`2c05e9e`](https://github.comgit//hawkw/mycelium/commit/2c05e9ecc9aaa061ab86569587529aa17a92e23a))
 * **[#197](https://github.comgit//hawkw/mycelium/issues/197)**
    - add some more cargo metadata ([`8fe36c4`](https://github.comgit//hawkw/mycelium/commit/8fe36c49d724e77711e42717044832c45db3ed34))
 * **Uncategorized**
    - cordyceps always exposes loom cfg ([`219fce1`](https://github.comgit//hawkw/mycelium/commit/219fce179e495bc25d083df53a132fe1dfee8305))
    - bump loom to 0.5.5, rm custom loom tracing ([`8b2ba7b`](https://github.comgit//hawkw/mycelium/commit/8b2ba7bdf5ec8182cc23ec424f9f84e0ffdf1fb1))
</details>

<csr-unknown>
 fix typo: incosistent -> inconsistent improve MPSC queue docs fix broken links, add netlify config<csr-unknown/>

