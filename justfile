#!/usr/bin/env -S just --justfile
# justfile for mycelium
# see https://just.systems/man for more details


# Overrides the default Rust toolchain set in `rust-toolchain.toml`.
toolchain := ""

# crates which should be tested under Miri
miri-crates := "-p cordyceps -p mycelium-util"

# disables cargo nextest
no-nextest := ''

_cargo := "cargo" + if toolchain != "" { " +" + toolchain } else { "" }

_rustflags := env_var_or_default("RUSTFLAGS", "")

# default recipe to display help information
default:
    @echo "justfile for Mycelium"
    @echo "see https://just.systems/man for more details"
    @echo ""
    @just --list

# run all tests and checks for `crate` (or for the whole workspace)
preflight crate='': (lint crate) (test crate)

# run all tests (normal tests), loom, and miri) for `crate` or for the whole workspace.
test crate='': (test-host crate) (loom crate) (miri crate) (test-docs crate)
    if crate == '' { cargo inoculate test } else { }

# run host tests for `crate` (or for the whole workspace).
test-host crate='': _get-nextest
    {{ _cargo }} {{ _testcmd }} \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }} \
        {{ _test-profile }} \
        --all-features

# run kernel tests in QEMU.
test-kernel:
    {{ _cargo }} inoculate test

# run doctests for `crate` (or for the whole workspace).
test-docs crate='':
    {{ _cargo }} test --doc \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }} \
        --all-features

# run lints (clippy, rustfmt, and docs checks) for `crate`
lint crate='': && (check-docs crate)
    {{ _cargo }} clippy {{ if crate == '' { '--workspace' } else { '-p' } }} {{ crate }}
    {{ _cargo }} clippy-x64 {{ if crate == '' { '--workspace' } else { '-p' } }} {{ crate }}
    {{ _cargo }} fmt --check {{ if crate == '' { '--workspace' } else { '-p' } }} {{ crate }}

# check documentation links and test docs for `crate` (or the whole workspace)
check-docs crate='': (build-docs crate '--cfg docsrs -Dwarnings') (test-docs crate)

# open RustDoc documentation for `crate` (or for the whole workspace).
docs crate='': (build-docs)
    {{ _cargo }} doc \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }} \
        --no-deps --all-features \
        --open

# build RustDoc documentation for the workspace.
build-docs crate='' $RUSTDOCFLAGS='--cfg docsrs':
    {{ _cargo }} doc --no-deps --all-features --document-private-items \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }}

# run Miri, either for `crate` or for all crates with miri tests.
miri crate='' $MIRIFLAGS='-Zmiri-strict-provenance -Zmiri-disable-isolation' $PROPTEST_CASES='10' $RUSTFLAGS="-Zrandomize-layout": _get-nextest
    @echo "MIRIFLAGS=\"$MIRIFLAGS\""
    @echo "RUSTFLAGS=\"$RUSTFLAGS\""
    @echo "PROPTEST_CASES=$PROPTEST_CASES"
    {{ _cargo }} miri {{ _testcmd }} \
        {{ if crate == '' { miri-crates } else { '-p' } }} {{ crate }} \
        {{ _test-profile }} \
        --lib

loom crate='' *args='': _get-nextest
    #!/usr/bin/env bash
    set -euo pipefail
    source "./bin/_util.sh"

    export RUSTFLAGS="--cfg loom ${RUSTFLAGS:-}"
    export LOOM_MAX_PREEMPTIONS="${LOOM_MAX_PREEMPTIONS:-2}"
    export LOOM_LOG="${LOOM_LOG:-mycelium=trace,maitake=trace,cordyceps=trace,debug}"

    # if logging is enabled, also enable location tracking.
    if [[ "${LOOM_LOG}" != "off" ]]; then
        export LOOM_LOCATION=true
        status "Enabled" "logging, LOOM_LOG=${LOOM_LOG}"
    else
        status "Disabled" "logging and location tracking"
    fi

    status "Configured" "loom, LOOM_MAX_PREEMPTIONS=${LOOM_MAX_PREEMPTIONS}"

    if [[ "${LOOM_CHECKPOINT_FILE:-}" ]]; then
        export LOOM_CHECKPOINT_FILE="${LOOM_CHECKPOINT_FILE:-}"
        export LOOM_CHECKPOINT_INTERVAL="${LOOM_CHECKPOINT_INTERVAL:-100}"
        status "Saving" "checkpoints to ${LOOM_CHECKPOINT_FILE} every ${LOOM_CHECKPOINT_INTERVAL} iterations"
    fi

    # if the loom tests fail, we still want to be able to print the checkpoint
    # location before exiting.
    set +e

    # run loom tests
    {{ _cargo }} {{ _testcmd }} \
        {{ _loom-profile }} \
        --lib \
        {{ if crate == '' { '-p maitake -p cordyceps -p mycelium-util'} else { '--package' } }} {{ crate }} \
        {{ args }}
    status="$?"

    if [[ "${LOOM_CHECKPOINT_FILE:-}" ]]; then
        status "Checkpoints" "saved to ${LOOM_CHECKPOINT_FILE}"
    fi

    exit "$status"

_loom-profile := if env_var_or_default("GITHUB_ACTION", "") != '' {
        '--profile loom-ci --cargo-profile loom'
    } else if no-nextest == '' {
        '--profile loom --cargo-profile loom'
    } else {
        '--profile loom'
    }

_test-profile := if env_var_or_default("GITHUB_ACTION", "") != '' {
        '--profile ci'
    } else {
        ''
    }

_testcmd := if no-nextest == '' {
        'nextest run'
    } else {
        'test'
    }

_get-nextest:
    #!/usr/bin/env bash
    set -euo pipefail
    source "./bin/_util.sh"

    if [ -n "{{ no-nextest }}" ]; then
        status "Configured" "not to use cargo nextest"
        exit 0
    fi

    if {{ _cargo }} --list | grep -q 'nextest'; then
        status "Found" "cargo nextest"
        exit 0
    fi

    err "missing cargo-nextest executable"
    if confirm "      install it?"; then
        cargo install cargo-nextest
    fi
