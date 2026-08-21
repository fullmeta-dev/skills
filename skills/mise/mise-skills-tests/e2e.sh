#!/usr/bin/env bash
# Clean isolated mise-skills lifecycle E2E. The compatibility harness separately
# proves bootstrap/plugin behavior across supported mise releases.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BIN="${MISE_SKILLS_BIN:-}"
KEEP_TEMP="${MISE_SKILLS_KEEP_TEMP:-}"
if [[ -z "$BIN" ]]; then
    BIN="$SCRIPT_DIR/../mise-skills/target/debug/mise-skills"
fi
if [[ ! -x "$BIN" ]]; then
    printf 'mise-skills binary is required; run cargo build or set MISE_SKILLS_BIN\n' >&2
    exit 2
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mise-skills-e2e.XXXXXX")"
cleanup() {
    if [[ "$KEEP_TEMP" == 1 ]]; then
        printf 'preserving E2E fixture at %s\n' "$WORK_DIR"
    else
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

HOME_DIR="$WORK_DIR/home"
CONFIG_DIR="$WORK_DIR/config"
DATA_DIR="$WORK_DIR/data"
STATE_DIR="$WORK_DIR/state"
CACHE_DIR="$WORK_DIR/cache"
PROJECT_DIR="$WORK_DIR/project"
SOURCE_DIR="$WORK_DIR/source"
FAKE_MISE="$WORK_DIR/fake-mise"
mkdir -p "$HOME_DIR" "$CONFIG_DIR" "$DATA_DIR" "$STATE_DIR" "$CACHE_DIR" \
    "$PROJECT_DIR" "$SOURCE_DIR/skills/demo"

export HOME="$HOME_DIR"
export MISE_CONFIG_DIR="$CONFIG_DIR"
export MISE_DATA_DIR="$DATA_DIR"
export MISE_STATE_DIR="$STATE_DIR"
export MISE_CACHE_DIR="$CACHE_DIR"
export MISE_BIN="$FAKE_MISE"

cat > "$FAKE_MISE" <<'EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
if [[ "${1:-}" == uninstall ]]; then
    exit 0
fi
printf 'unexpected fake mise call: %s\n' "$*" >&2
exit 1
EOF
chmod +x "$FAKE_MISE"

cat > "$SOURCE_DIR/skills/demo/SKILL.md" <<'EOF'
---
name: demo
description: E2E demo skill.
tags: [web]
---
# Demo
EOF
printf 'initial\n' > "$SOURCE_DIR/skills/demo/reference.txt"

run() {
    "$BIN" "$@"
}

copy_artifact() {
    local version="$1"
    local destination="$DATA_DIR/installs/skills-local-source/$version"
    mkdir -p "$destination"
    cp -a "$SOURCE_DIR/." "$destination/"
}

version_from_config() {
    sed -nE 's/^version = "([^"]+)"/\1/p' "$PROJECT_DIR/mise.toml" | head -n1
}

printf '1/7 bootstrap/task metadata and source configuration\n'
cat > "$CONFIG_DIR/config.toml" <<'EOF'
[tasks.skills]
description = "Manage Agent Skills"
usage = '''
arg "[arguments]" var=#true help="mise-skills command and arguments"
'''
run = "mise-skills"
raw = true
EOF
run use --path "$PROJECT_DIR/mise.toml" "path:$SOURCE_DIR" demo \
    --agent agents --agent codex --agent cursor --agent pi --agent claude
[[ -f "$PROJECT_DIR/mise.toml" ]]
grep -Fq 'mise run skills -- sync --locked' "$PROJECT_DIR/mise.toml"

printf '2/7 immutable install and discovery\n'
PINNED="$(version_from_config)"
copy_artifact "$PINNED"
run list "$SOURCE_DIR" --json > "$WORK_DIR/list.json"
grep -Fq '"demo"' "$WORK_DIR/list.json"

printf '3/7 adapter activation and status\n'
run sync --path "$PROJECT_DIR/mise.toml" --locked
for target in \
    "$PROJECT_DIR/.agents/skills/demo" \
    "$PROJECT_DIR/.pi/skills/demo" \
    "$PROJECT_DIR/.claude/skills/demo"; do
    [[ -L "$target" ]]
done
run status --path "$PROJECT_DIR/mise.toml" --json > "$WORK_DIR/status.json"
grep -Fq '"active"' "$WORK_DIR/status.json"

printf '4/7 locked drift rejection and explicit upgrade\n'
printf 'changed\n' >> "$SOURCE_DIR/skills/demo/reference.txt"
if run sync --path "$PROJECT_DIR/mise.toml" --locked >"$WORK_DIR/drift.out" 2>&1; then
    printf 'locked sync unexpectedly accepted local drift\n' >&2
    exit 1
fi
grep -Fq 'local source changed' "$WORK_DIR/drift.out"
run upgrade --path "$PROJECT_DIR/mise.toml"
UPGRADED="$(version_from_config)"
[[ "$UPGRADED" != "$PINNED" ]]
copy_artifact "$UPGRADED"
run sync --path "$PROJECT_DIR/mise.toml" --locked

printf '5/7 recovery and ownership conflict\n'
rm "$PROJECT_DIR/.pi/skills/demo"
run sync --path "$PROJECT_DIR/mise.toml" --locked
after_recovery="$PROJECT_DIR/.pi/skills/demo"
[[ -L "$after_recovery" ]]
rm "$after_recovery"
printf 'unmanaged\n' > "$after_recovery"
run sync --path "$PROJECT_DIR/mise.toml" --locked >"$WORK_DIR/conflict.out" 2>&1
grep -Fq 'conflict' "$WORK_DIR/conflict.out"
[[ "$(cat "$after_recovery")" == unmanaged ]]
rm "$after_recovery"
run prune --path "$PROJECT_DIR/mise.toml"

printf '6/7 selected removal and artifact delegation\n'
run remove --path "$PROJECT_DIR/mise.toml" skills:local/source demo
! grep -Fq 'skills:local/source' "$PROJECT_DIR/mise.toml"
[[ ! -e "$PROJECT_DIR/.agents/skills/demo" ]]
[[ ! -e "$PROJECT_DIR/.claude/skills/demo" ]]

printf '7/7 global scope and convergence\n'
run use --global "path:$SOURCE_DIR" demo --agent agents
GLOBAL_CONFIG="$CONFIG_DIR/config.toml"
[[ -f "$GLOBAL_CONFIG" ]]
GLOBAL_PIN="$(grep -A3 'skills:local/source' "$GLOBAL_CONFIG" | sed -nE 's/^version = "([^"]+)"/\1/p' | head -n1)"
copy_artifact "$GLOBAL_PIN"
run sync --global --locked
[[ -L "$HOME_DIR/.agents/skills/demo" ]]
run remove --global skills:local/source
[[ ! -e "$HOME_DIR/.agents/skills/demo" ]]

printf 'isolated mise-skills E2E passed\n'
