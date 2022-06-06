#!/usr/bin/env bash
git-cliff \
    --workdir "$WORKSPACE_ROOT" \
    --include-path "$CRATE_ROOT/**" \
    --prepend "$CRATE_ROOT/CHANGELOG.md" \
    --latest \
    --tag "$1"