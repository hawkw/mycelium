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

# If we're running in Github Actions and cargo-action-fmt is installed, then add
# a command suffix that formats errors.
_fmt := if env_var_or_default("GITHUB_ACTIONS", "") != "true" { "" } else {
    ```
    if command -v cargo-action-fmt >/dev/null 2>&1; then
        echo "--message-format=json | cargo-action-fmt"
    fi
    ```
}

# arguments to pass to all RustDoc invocations
_rustdoc := _cargo + " doc --no-deps --all-features --document-private-items"

# default recipe to display help information
default:
    @echo "justfile for Mycelium"
    @echo "see https://just.systems/man for more details"
    @echo ""
    @just --list

# run all tests and checks for `crate` (or for the whole workspace)
preflight crate='': (lint crate) (test crate)

# run all tests (normal tests, loom, and miri) for `crate` or for the whole workspace.
test crate='': (test-host crate) (loom crate) (miri crate) (test-docs crate)
    if crate == '' { _cargo inoculate test } else { }

# run host tests for `crate` (or for the whole workspace).
test-host crate='': _get-nextest
    {{ _cargo }} build --tests --all-features \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }} \
        {{ _fmt }}
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
lint crate='': (clippy crate) (check-fmt crate) (check-docs crate)

# run clippy lints for `crate`
clippy crate='':
    {{ _cargo }} clippy \
        {{ if crate == '' { '--workspace' } else { '-p' } }} {{ crate }} \
        {{ _fmt }}
    {{ if crate == '' { _clippy-x64 } else if crate == 'mycelium-kernel' { _clippy-x64 } else { '' } }}

# check rustfmt for `crate`
check-fmt crate='':
    {{ _cargo }} fmt --check \
        {{ if crate == '' { '--all' } else { '-p' } }} {{ crate }} \
        {{ _fmt }}

# check documentation links and test docs for `crate` (or the whole workspace)
check-docs crate='': (build-docs crate '--cfg docsrs -Dwarnings') (test-docs crate)

# open RustDoc documentation for `crate` (or for the whole workspace).
docs crate='' $RUSTDOCFLAGS='--cfg docsrs': (build-docs crate RUSTDOCFLAGS)
    {{ _rustdoc }} \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }} \
        --open

# build RustDoc documentation for the workspace.
build-docs crate='' $RUSTDOCFLAGS='--cfg docsrs':
    {{ _rustdoc }} \
        {{ if crate == '' { '--workspace' } else { '--package' } }} {{ crate }} \
        {{ _fmt }}

# run Miri, either for `crate` or for all crates with miri tests.
miri crate='' *args='': _get-nextest
    MIRIFLAGS="{{ env_var_or_default("MIRIFLAGS", "-Zmiri-strict-provenance -Zmiri-disable-isolation") }}" \
        RUSTFLAGS="{{ env_var_or_default("RUSTFLAGS", "-Zrandomize-layout") }}" \
        PROPTEST_CASES="{{ env_var_or_default("PROPTEST_CASES", "10") }}" \
        {{ _cargo }} miri {{ _testcmd }} \
        {{ if crate == '' { miri-crates } else { '-p' } }} {{ crate }} \
        {{ _test-profile }} \
        --lib \
        {{ args }}

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

_clippy-x64 := _cargo + " clippy-x64 -p mycelium-kernel " + _fmt

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
