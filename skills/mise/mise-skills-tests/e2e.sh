#!/usr/bin/env bash
# Clean isolated lifecycle E2E through real mise, the shipped backend plugin,
# and a real mise task. The local HTTP release fixture supplies only the
# normal mise-managed mise-skills executable.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd)"
PLUGIN_SOURCE="$REPO_ROOT/skills/mise/plugin"
BOOTSTRAP_HELPER="$REPO_ROOT/skills/mise/bootstrap-plugin.sh"
MISE_BIN="${MISE_BIN:-$(command -v mise || true)}"
CLI_BIN="${MISE_SKILLS_BIN:-$REPO_ROOT/skills/mise/mise-skills/target/debug/mise-skills}"
KEEP_TEMP="${MISE_SKILLS_KEEP_TEMP:-}"

if [[ -z "$MISE_BIN" || ! -x "$MISE_BIN" ]]; then
    printf 'mise is required; set MISE_BIN to an executable mise binary\n' >&2
    exit 2
fi
if [[ ! -x "$CLI_BIN" ]]; then
    printf 'mise-skills binary is required; run cargo build or set MISE_SKILLS_BIN\n' >&2
    exit 2
fi
if [[ ! -x "$BOOTSTRAP_HELPER" ]]; then
    printf 'shipped bootstrap helper is not executable: %s\n' "$BOOTSTRAP_HELPER" >&2
    exit 2
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mise-skills-e2e.XXXXXX")"
MISE_PID=""
cleanup() {
    if [[ -n "$MISE_PID" ]]; then
        kill "$MISE_PID" 2>/dev/null || true
        wait "$MISE_PID" 2>/dev/null || true
    fi
    if [[ "$KEEP_TEMP" == 1 ]]; then
        printf 'preserving E2E fixture at %s\n' "$WORK_DIR"
    else
        rm -rf "$WORK_DIR"
    fi
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
RELEASE_DIR="$WORK_DIR/release"
mkdir -p "$PLUGIN_REPO" "$SOURCE_DIR/skills/demo" "$HOME_DIR" "$CONFIG_DIR" \
    "$DATA_DIR" "$STATE_DIR" "$CACHE_DIR" "$PROJECT_DIR" "$RELEASE_DIR"

# Make a pinned Git repository from the shipped plugin. Real mise clones and
# installs this repository; no fake mise executable or direct CLI invocation is
# used anywhere in the lifecycle below.
cp -a "$PLUGIN_SOURCE/." "$PLUGIN_REPO/"
git -C "$PLUGIN_REPO" init -q
git -C "$PLUGIN_REPO" add .
git -C "$PLUGIN_REPO" -c user.email=e2e@example.invalid \
    -c user.name=e2e commit -qm shipped-plugin
PLUGIN_REF="$(git -C "$PLUGIN_REPO" rev-parse HEAD)"
PLUGIN_URL="file://$PLUGIN_REPO"

cat > "$SOURCE_DIR/skills/demo/SKILL.md" <<'EOF'
---
name: demo
description: E2E demo skill.
tags: [web]
---
# Demo
EOF
printf 'initial\n' > "$SOURCE_DIR/skills/demo/reference.txt"

# A local release fixture is deliberately limited to the binary asset. The
# GitHub backend still performs the normal release lookup and mise installs it
# as the global CLI tool before the task is invoked.
cp "$CLI_BIN" "$RELEASE_DIR/mise-skills-fixture"
chmod +x "$RELEASE_DIR/mise-skills-fixture"
cat > "$RELEASE_DIR/server.py" <<'PY'
import http.server
import json
import sys
from urllib.parse import urlparse

asset_path, port_path, request_log = sys.argv[1:]


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format_string, *args):
        with open(request_log, "a", encoding="utf-8") as log:
            log.write(f"{self.command} {self.path}\n")

    def do_GET(self):
        path = urlparse(self.path).path
        port = server.server_address[1]
        asset_url = f"http://127.0.0.1:{port}/assets/mise-skills-fixture"
        asset = {
            "name": "mise-skills-fixture",
            "browser_download_url": asset_url,
            "url": asset_url,
        }
        release = {
            "tag_name": "v0.1.0",
            "draft": False,
            "prerelease": False,
            "created_at": "2026-01-01T00:00:00Z",
            "published_at": "2026-01-01T00:00:00Z",
            "assets": [asset],
        }
        if "/releases/tags/" in path:
            body = json.dumps(release).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if path.endswith("/releases") or path.endswith("/releases/latest"):
            body = json.dumps([release]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if path == "/assets/mise-skills-fixture":
            with open(asset_path, "rb") as asset:
                body = asset.read()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        body = b"not found"
        self.send_response(404)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(port_path, "w", encoding="ascii") as port:
    port.write(str(server.server_address[1]))
server.serve_forever()
PY
: > "$RELEASE_DIR/requests.log"
python3 -u "$RELEASE_DIR/server.py" "$RELEASE_DIR/mise-skills-fixture" \
    "$RELEASE_DIR/port" "$RELEASE_DIR/requests.log" >"$RELEASE_DIR/server.log" 2>&1 &
MISE_PID=$!
for _ in {1..50}; do
    [[ -s "$RELEASE_DIR/port" ]] && break
    sleep 0.1
done
[[ -s "$RELEASE_DIR/port" ]]
RELEASE_API="http://127.0.0.1:$(cat "$RELEASE_DIR/port")"

export HOME="$HOME_DIR"
export MISE_CONFIG_DIR="$CONFIG_DIR"
export MISE_DATA_DIR="$DATA_DIR"
export MISE_STATE_DIR="$STATE_DIR"
export MISE_CACHE_DIR="$CACHE_DIR"
export MISE_TRUSTED_CONFIG_PATHS="$CONFIG_DIR:$PROJECT_DIR"
export MISE_BIN
export MISE_SKILLS_GITHUB_BASE_URL="file://$WORK_DIR/github"
cd "$PROJECT_DIR"

mise_cmd() {
    "$MISE_BIN" "$@"
}
run_skills() {
    mise_cmd run skills -- "$@"
}

cat > "$CONFIG_DIR/config.toml" <<EOF
[settings]
experimental = true

[plugins]
"vfox-backend:skills" = "$PLUGIN_URL#$PLUGIN_REF"

[tools."github:fullmeta-dev/skills"]
version = "0.1.0"
bin = "mise-skills"
asset_pattern = "mise-skills-fixture"
api_url = "$RELEASE_API"

[tasks.skills]
description = "Manage Agent Skills"
usage = '''
arg "[arguments]" var=#true help="mise-skills command and arguments"
'''
dir = "{{cwd}}"
run = "mise-skills \${usage_arguments-}"
raw = true

[bootstrap.hooks.pre-tools]
run = "bash $BOOTSTRAP_HELPER skills $PLUGIN_URL $PLUGIN_REF"
EOF

printf '1/8 real bootstrap installs the pinned shipped plugin\n'
mise_cmd bootstrap --only tools --yes >"$WORK_DIR/bootstrap.log" 2>&1 || {
    cat "$WORK_DIR/bootstrap.log" >&2
    exit 1
}
[[ "$(git -C "$DATA_DIR/plugins/skills" rev-parse HEAD)" == "$PLUGIN_REF" ]]

printf '2/8 real mise installs the CLI release fixture and task metadata\n'
mise_cmd install --jobs=1 >"$WORK_DIR/cli-install.log" 2>&1 || {
    cat "$WORK_DIR/cli-install.log" >&2
    exit 1
}
mise_cmd tasks info skills --json >"$WORK_DIR/task.json"
grep -Fq '"description": "Manage Agent Skills"' "$WORK_DIR/task.json"
grep -Fq '"usage_spec"' "$WORK_DIR/task.json"
grep -Fq '/releases/tags/v0.1.0' "$RELEASE_DIR/requests.log"

printf '3/8 task help and CLI help remain side-effect free\n'
mise_cmd run skills --help >"$WORK_DIR/task-help.txt"
run_skills --help >"$WORK_DIR/cli-help.txt"
grep -Fq 'Manage Agent Skills' "$WORK_DIR/task-help.txt"
grep -Fq 'Normal behavior:' "$WORK_DIR/cli-help.txt"

printf '4/8 task configures a project, then real mise installs the backend artifact\n'
SOURCE_ARG="path:$SOURCE_DIR"
run_skills use "$SOURCE_ARG" demo --agent agents --agent codex --agent cursor \
    --agent pi --agent claude
[[ -f "$PROJECT_DIR/mise.toml" ]]
grep -Fq 'mise run skills -- sync --locked' "$PROJECT_DIR/mise.toml"
mise_cmd install --jobs=1 >"$WORK_DIR/project-install.log" 2>&1 || {
    cat "$WORK_DIR/project-install.log" >&2
    exit 1
}

printf '5/8 discovery, validation, adapters, and status use the real task\n'
run_skills list "$SOURCE_DIR" --tree --explain >"$WORK_DIR/list.txt"
grep -Fq 'skills/demo' "$WORK_DIR/list.txt"
run_skills list "$SOURCE_DIR" --include tag:web --exclude tag:experimental --json --explain \
    >"$WORK_DIR/list.json"
grep -Fq '"demo"' "$WORK_DIR/list.json"
run_skills validate "$SOURCE_DIR" --json >"$WORK_DIR/validate.json"
grep -Fq 'intentional extension' "$WORK_DIR/validate.json"
run_skills validate "$SOURCE_DIR" >"$WORK_DIR/validate.txt"
grep -Fq 'non-blocking' "$WORK_DIR/validate.txt"
run_skills sync --locked
for target in \
    "$PROJECT_DIR/.agents/skills/demo" \
    "$PROJECT_DIR/.pi/skills/demo" \
    "$PROJECT_DIR/.claude/skills/demo"; do
    [[ -L "$target" ]]
done
run_skills status --json >"$WORK_DIR/status.json"
grep -Fq '"active"' "$WORK_DIR/status.json"

printf '6/8 local drift fails locked sync and explicit upgrade repairs it\n'
printf 'changed\n' >> "$SOURCE_DIR/skills/demo/reference.txt"
if run_skills sync --locked >"$WORK_DIR/drift.out" 2>&1; then
    printf 'locked sync unexpectedly accepted local drift\n' >&2
    exit 1
fi
grep -Fq 'local source changed' "$WORK_DIR/drift.out"
run_skills upgrade
mise_cmd install --jobs=1 >"$WORK_DIR/upgrade-install.log" 2>&1 || {
    cat "$WORK_DIR/upgrade-install.log" >&2
    exit 1
}
run_skills sync --locked

printf '7/8 recovery, conflict reporting, removal, and prune\n'
rm "$PROJECT_DIR/.pi/skills/demo"
run_skills sync --locked
[[ -L "$PROJECT_DIR/.pi/skills/demo" ]]
rm "$PROJECT_DIR/.pi/skills/demo"
printf 'unmanaged\n' > "$PROJECT_DIR/.pi/skills/demo"
run_skills sync --locked >"$WORK_DIR/conflict.out" 2>&1
grep -Fq 'conflict' "$WORK_DIR/conflict.out"
[[ "$(cat "$PROJECT_DIR/.pi/skills/demo")" == unmanaged ]]
rm "$PROJECT_DIR/.pi/skills/demo"
run_skills prune
run_skills remove skills:local/source demo
! grep -Fq 'skills:local/source' "$PROJECT_DIR/mise.toml"
[[ ! -e "$PROJECT_DIR/.agents/skills/demo" ]]
[[ ! -e "$PROJECT_DIR/.claude/skills/demo" ]]

printf '8/8 global scope converges through the same real task\n'
run_skills use --global "$SOURCE_ARG" demo --agent agents
mise_cmd install --jobs=1 >"$WORK_DIR/global-install.log" 2>&1 || {
    cat "$WORK_DIR/global-install.log" >&2
    exit 1
}
run_skills sync --global --locked
[[ -L "$HOME_DIR/.agents/skills/demo" ]]
run_skills status --global --json >"$WORK_DIR/global-status.json"
grep -Fq '"active"' "$WORK_DIR/global-status.json"
run_skills remove --global skills:local/source
[[ ! -e "$HOME_DIR/.agents/skills/demo" ]]

printf 'real mise/task/backend/plugin lifecycle E2E passed\n'
