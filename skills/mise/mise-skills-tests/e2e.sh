#!/usr/bin/env bash
# Hermetic lifecycle through a real mise process and the public skills binary.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd)"
PLUGIN_SOURCE="$REPO_ROOT/skills/mise/plugin"
MISE_BIN="${MISE_BIN:-$(command -v mise || true)}"
CLI_BIN="${MISE_SKILLS_BIN:-$REPO_ROOT/skills/mise/mise-skills/target/debug/skills}"
[[ -x "$MISE_BIN" ]] || { printf 'mise is required\n' >&2; exit 2; }
[[ -x "$CLI_BIN" ]] || { printf 'skills binary is required\n' >&2; exit 2; }

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/skills-e2e.XXXXXX")"
KEEP_TEMP="${MISE_SKILLS_KEEP_TEMP:-}"
cleanup() {
    if [[ "$KEEP_TEMP" == 1 ]]; then printf 'preserving E2E fixture at %s\n' "$WORK_DIR"; else rm -rf "$WORK_DIR"; fi
}
trap cleanup EXIT

PLUGIN_REPO="$WORK_DIR/plugin"
SOURCE_DIR="$WORK_DIR/source"
HOME_DIR="$WORK_DIR/home"
CONFIG_DIR="$WORK_DIR/config"
DATA_DIR="$WORK_DIR/data"
STATE_DIR="$WORK_DIR/state"
CACHE_DIR="$WORK_DIR/cache"
PROJECT_DIR="$WORK_DIR/project"
mkdir -p "$PLUGIN_REPO" "$SOURCE_DIR/skills/demo" "$HOME_DIR" "$CONFIG_DIR" \
    "$DATA_DIR" "$STATE_DIR" "$CACHE_DIR" "$PROJECT_DIR"
cp -a "$PLUGIN_SOURCE/." "$PLUGIN_REPO/"
git -C "$PLUGIN_REPO" init -q
git -C "$PLUGIN_REPO" add .
git -C "$PLUGIN_REPO" -c user.email=e2e@example.invalid -c user.name=e2e commit -qm plugin
PLUGIN_REF="$(git -C "$PLUGIN_REPO" rev-parse HEAD)"

cat > "$SOURCE_DIR/skills/demo/SKILL.md" <<'EOF'
---
name: demo
description: E2E demo skill.
tags: [web]
---
# Demo
EOF
printf 'fixture\n' > "$SOURCE_DIR/skills/demo/reference.txt"

export HOME="$HOME_DIR" MISE_CONFIG_DIR="$CONFIG_DIR" MISE_DATA_DIR="$DATA_DIR"
export MISE_STATE_DIR="$STATE_DIR" MISE_CACHE_DIR="$CACHE_DIR" MISE_BIN
export MISE_TRUSTED_CONFIG_PATHS="$CONFIG_DIR:$PROJECT_DIR"
export PATH="$(dirname "$CLI_BIN"):$PATH"
cd "$PROJECT_DIR"

"$MISE_BIN" plugins install skills "file://$PLUGIN_REPO#$PLUGIN_REF" >/dev/null
cat > "$PROJECT_DIR/mise.toml" <<EOF
[plugins]
"vfox-backend:skills" = "file://$PLUGIN_REPO#$PLUGIN_REF"
EOF
SOURCE_ARG="path:$SOURCE_DIR"
"$CLI_BIN" use --path "$PROJECT_DIR/mise.toml" "$SOURCE_ARG" demo --agent agents --agent pi --agent claude
[[ -f "$PROJECT_DIR/mise.toml" ]]
grep -Fq 'skills sync --locked' "$PROJECT_DIR/mise.toml"

printf 'public binary and normal mise install\n'
"$CLI_BIN" install --path "$PROJECT_DIR/mise.toml"
for target in "$PROJECT_DIR/.agents/skills/demo" "$PROJECT_DIR/.pi/skills/demo" "$PROJECT_DIR/.claude/skills/demo"; do [[ -L "$target" ]]; done
"$CLI_BIN" status --path "$PROJECT_DIR/mise.toml" --json | grep -Fq '"active"'

printf 'locked local drift and explicit source upgrade\n'
printf 'drift\n' >> "$SOURCE_DIR/skills/demo/reference.txt"
if "$CLI_BIN" sync --path "$PROJECT_DIR/mise.toml" --locked >/dev/null 2>&1; then
    printf 'locked sync accepted local drift\n' >&2; exit 1
fi
"$CLI_BIN" upgrade --path "$PROJECT_DIR/mise.toml"
"$CLI_BIN" install --path "$PROJECT_DIR/mise.toml"

printf 'environment-specific config target\n'
"$CLI_BIN" use --path "$PROJECT_DIR/mise.toml" --env dev "$SOURCE_ARG" demo --agent pi
[[ -f "$PROJECT_DIR/mise.dev.toml" ]]
"$CLI_BIN" install --path "$PROJECT_DIR/mise.toml" --env dev

printf 'executable collision is diagnosed without normal mise install\n'
FOREIGN_BIN="$WORK_DIR/foreign-bin"
mkdir -p "$FOREIGN_BIN"
printf '#!/bin/sh\n' > "$FOREIGN_BIN/skills"
chmod +x "$FOREIGN_BIN/skills"
if PATH="$FOREIGN_BIN:$(dirname "$CLI_BIN"):$PATH" "$CLI_BIN" install --path "$PROJECT_DIR/mise.toml" > /tmp/skills-bin-collision.$$ 2>&1; then
    printf 'executable collision unexpectedly succeeded\n' >&2; exit 1
fi
grep -Fq 'PATH owner:' /tmp/skills-bin-collision.$$
rm -f /tmp/skills-bin-collision.$$

printf 'link collision is diagnosed without replacing the foreign file\n'
rm -f "$PROJECT_DIR/.pi/skills/demo"
printf 'foreign\n' > "$PROJECT_DIR/.pi/skills/demo"
"$CLI_BIN" install --path "$PROJECT_DIR/mise.toml" >/tmp/skills-collision.$$ 2>&1
grep -Fq 'conflict' /tmp/skills-collision.$$
[[ "$(cat "$PROJECT_DIR/.pi/skills/demo")" == foreign ]]
rm -f /tmp/skills-collision.$$

printf 'real mise skills lifecycle E2E passed\n'
