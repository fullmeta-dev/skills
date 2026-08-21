#!/usr/bin/env bash
# Hermetic real-mise release and lifecycle checks.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd)"
MISE_BIN="${MISE_BIN:-$(command -v mise || true)}"
if [[ -z "$MISE_BIN" || ! -x "$MISE_BIN" ]]; then
    printf 'mise is required; set MISE_BIN to an executable mise binary\n' >&2
    exit 2
fi

if [[ ! -x "$REPO_ROOT/skills/mise/mise-skills/target/debug/skills" ]]; then
    cargo build --manifest-path "$REPO_ROOT/skills/mise/mise-skills/Cargo.toml"
fi

MISE_BIN="$MISE_BIN" MISE_SKILLS_BIN="$REPO_ROOT/skills/mise/mise-skills/target/debug/skills" \
    "$SCRIPT_DIR/backend_mise_test.sh"
MISE_BIN="$MISE_BIN" MISE_SKILLS_BIN="$REPO_ROOT/skills/mise/mise-skills/target/debug/skills" \
    "$SCRIPT_DIR/e2e.sh"
printf 'real-mise skills checks passed\n'
