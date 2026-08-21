#!/usr/bin/env bash
# Disposable Milestone 0 compatibility harness for the documented mise APIs.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
MINIMUM_VERSION="${MISE_SKILLS_MINIMUM_MISE_VERSION:-2026.6.14}"
LATEST_VERSION_OVERRIDE="${MISE_SKILLS_LATEST_MISE_VERSION:-}"
MINIMUM_BINARY_OVERRIDE="${MISE_SKILLS_MINIMUM_MISE_BIN:-}"
LATEST_BINARY_OVERRIDE="${MISE_SKILLS_LATEST_MISE_BIN:-}"
OLDER_VERSION_OVERRIDE="${MISE_SKILLS_OLDER_MISE_VERSION:-2026.6.13}"
OLDER_BINARY_OVERRIDE="${MISE_SKILLS_OLDER_MISE_BIN:-}"
KEEP_TEMP="${MISE_SKILLS_KEEP_TEMP:-}"
CURRENT_MISE="${MISE_BIN:-$(command -v mise || true)}"
# These names are intentionally not passed to mise: mise treats MISE_* variables
# as settings and would reject harness-only names while loading a fixture.
unset MISE_BIN MISE_SKILLS_MINIMUM_MISE_VERSION MISE_SKILLS_LATEST_MISE_VERSION \
    MISE_SKILLS_MINIMUM_MISE_BIN MISE_SKILLS_LATEST_MISE_BIN \
    MISE_SKILLS_OLDER_MISE_VERSION MISE_SKILLS_OLDER_MISE_BIN MISE_SKILLS_KEEP_TEMP

usage() {
    cat <<'EOF'
Usage: run.sh

Run the six Milestone 0 compatibility checks in isolated HOME, config, data,
state, and cache directories against the pinned minimum and latest stable mise.

Environment overrides:
  MISE_BIN                         mise binary used when it is the minimum
  MISE_SKILLS_MINIMUM_MISE_VERSION minimum supported release (default: 2026.6.14)
  MISE_SKILLS_LATEST_MISE_VERSION  latest release to test (otherwise discovered)
  MISE_SKILLS_MINIMUM_MISE_BIN     binary to use for the minimum release
  MISE_SKILLS_LATEST_MISE_BIN      binary to use for the latest release
  MISE_SKILLS_OLDER_MISE_VERSION   older release for the rejection check
  MISE_SKILLS_OLDER_MISE_BIN       binary to use for the older release
  MISE_SKILLS_KEEP_TEMP=1          preserve the disposable test directories
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi
if [[ $# -ne 0 ]]; then
    printf 'unexpected argument: %s\n' "$1" >&2
    usage >&2
    exit 2
fi
if [[ -z "$CURRENT_MISE" || ! -x "$CURRENT_MISE" ]]; then
    printf 'mise is required; set MISE_BIN to an executable mise binary\n' >&2
    exit 2
fi

version_of() {
    local output
    output="$(MISE_NO_CONFIG=1 "$1" --version 2>/dev/null || true)"
    printf '%s\n' "$output" | sed -nE 's/^([0-9]+\.[0-9]+\.[0-9]+).*/\1/p' | head -n1
}

normalise_version() {
    printf '%s\n' "${1#v}"
}

latest_version() {
    if [[ -n "$LATEST_VERSION_OVERRIDE" ]]; then
        normalise_version "$LATEST_VERSION_OVERRIDE"
        return
    fi

    local discovered
    discovered="$(MISE_NO_CONFIG=1 "$CURRENT_MISE" version -J 2>/dev/null \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["latest"])' 2>/dev/null || true)"
    if [[ -n "$discovered" ]]; then
        normalise_version "$discovered"
        return
    fi

    curl -fsSL https://api.github.com/repos/jdx/mise/releases/latest \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"].lstrip("v"))'
}

asset_name() {
    local os arch
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) printf 'unsupported host OS: %s\n' "$(uname -s)" >&2; return 1 ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) arch=x64 ;;
        aarch64|arm64) arch=arm64 ;;
        armv7l) arch=armv7 ;;
        *) printf 'unsupported host architecture: %s\n' "$(uname -m)" >&2; return 1 ;;
    esac
    printf 'mise-v%s-%s-%s\n' "$1" "$os" "$arch"
}

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mise-skills-compat.XXXXXX")"
GITHUB_FIXTURE_PID=""
stop_github_fixture() {
    if [[ -n "$GITHUB_FIXTURE_PID" ]]; then
        kill "$GITHUB_FIXTURE_PID" 2>/dev/null || true
        wait "$GITHUB_FIXTURE_PID" 2>/dev/null || true
        GITHUB_FIXTURE_PID=""
    fi
}
cleanup() {
    stop_github_fixture
    if [[ "$KEEP_TEMP" == 1 ]]; then
        printf 'preserving compatibility fixtures at %s\n' "$WORK_DIR"
    else
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

CACHE_DIR="$WORK_DIR/mise-binaries"
mkdir -p "$CACHE_DIR"

resolve_binary() {
    local version override asset destination url current_version
    version="$(normalise_version "$1")"
    override="${2:-}"
    if [[ -n "$override" ]]; then
        [[ -x "$override" ]] || { printf 'mise binary is not executable: %s\n' "$override" >&2; return 1; }
        printf '%s\n' "$override"
        return
    fi

    current_version="$(version_of "$CURRENT_MISE")"
    if [[ "$current_version" == "$version" ]]; then
        printf '%s\n' "$CURRENT_MISE"
        return
    fi

    asset="$(asset_name "$version")"
    destination="$CACHE_DIR/$asset"
    if [[ ! -x "$destination" ]]; then
        url="https://github.com/jdx/mise/releases/download/v$version/$asset"
        printf 'downloading mise %s from %s\n' "$version" "$url" >&2
        curl --fail --location --silent --show-error "$url" --output "$destination"
        chmod +x "$destination"
    fi
    printf '%s\n' "$destination"
}

PLUGIN_DIR="$WORK_DIR/plugin"
PLUGIN_URL="file://$PLUGIN_DIR"
PLUGIN_REF_ONE=""
PLUGIN_REF_TWO=""

make_plugin_repo() {
    cp -R "$SCRIPT_DIR/plugin" "$PLUGIN_DIR"
    git -C "$PLUGIN_DIR" init -q
    git -C "$PLUGIN_DIR" add .
    git -C "$PLUGIN_DIR" -c user.email=compat@example.invalid -c user.name=compat commit -qm initial
    PLUGIN_REF_ONE="$(git -C "$PLUGIN_DIR" rev-parse HEAD)"

    local metadata_tmp="$PLUGIN_DIR/metadata.lua.tmp"
    sed 's/0.1.0-compat/0.1.0-compat-2/' "$PLUGIN_DIR/metadata.lua" > "$metadata_tmp"
    mv "$metadata_tmp" "$PLUGIN_DIR/metadata.lua"
    git -C "$PLUGIN_DIR" add metadata.lua
    git -C "$PLUGIN_DIR" -c user.email=compat@example.invalid -c user.name=compat commit -qm replacement
    PLUGIN_REF_TWO="$(git -C "$PLUGIN_DIR" rev-parse HEAD)"
}

CASE_DIR=""
CASE_BIN=""
LOG_FILE=""

begin_case() {
    local name="$1" binary="$2"
    CASE_DIR="$WORK_DIR/cases/$name"
    mkdir -p "$CASE_DIR"/{home,config,data,state,cache,project,bin}
    ln -s "$binary" "$CASE_DIR/bin/mise"
    CASE_BIN="$CASE_DIR/bin/mise"
    LOG_FILE="$CASE_DIR/compat.log"
    : > "$LOG_FILE"

    export HOME="$CASE_DIR/home"
    export MISE_CONFIG_DIR="$CASE_DIR/config"
    export MISE_DATA_DIR="$CASE_DIR/data"
    export MISE_STATE_DIR="$CASE_DIR/state"
    export MISE_CACHE_DIR="$CASE_DIR/cache"
    export MISE_TRUSTED_CONFIG_PATHS="$CASE_DIR/project:$CASE_DIR/config"
    export SKILLS_COMPAT_LOG="$LOG_FILE"
    export PATH="$CASE_DIR/bin:${ORIGINAL_PATH:-$PATH}"
    cd "$CASE_DIR/project"
}

run_mise() {
    "$CASE_BIN" "$@"
}

assert_file_contains() {
    local needle="$1" file="$2"
    grep -Fqx -- "$needle" "$file" || {
        printf 'expected %q in %s\n' "$needle" "$file" >&2
        cat "$file" >&2
        return 1
    }
}

shell_quote() {
    local value="$1"
    printf "'%s'" "$(printf '%s' "$value" | sed "s/'/'\\\\''/g")"
}

bootstrap_command() {
    printf 'bash %s %s %s %s' \
        "$(shell_quote "$SCRIPT_DIR/bootstrap-plugin.sh")" \
        "$(shell_quote skills)" "$(shell_quote "$PLUGIN_URL")" \
        "$(shell_quote "$PLUGIN_REF_ONE")"
}

minimum_command() {
    printf 'bash %s %s %s' \
        "$(shell_quote "$SCRIPT_DIR/ensure-minimum.sh")" \
        "$(shell_quote mise)" "$(shell_quote "$MINIMUM_VERSION")"
}

write_bootstrap_config() {
    cat > mise.toml <<EOF
[settings]
experimental = true

[plugins]
"vfox-backend:skills" = "$PLUGIN_URL#$PLUGIN_REF_ONE"

[tools."skills:source"]
version = "0.0.0"

[bootstrap.hooks.pre-tools]
run = "$(minimum_command) && $(bootstrap_command)"
EOF
}

run_lifecycle() {
    printf '  1/6 backend plugin lifecycle\n'
    run_mise plugins install skills "$PLUGIN_URL#$PLUGIN_REF_ONE" >/dev/null
    assert_file_contains "skills" <(run_mise plugins ls)
    run_mise ls-remote skills:source | grep -Fq '0.0.0'
    cat > mise.toml <<'EOF'
[tools."skills:source"]
version = "0.0.0"
EOF
    run_mise install --jobs=1 >/dev/null
    grep -Fq '"event":"backend-install"' "$LOG_FILE"
}

run_bootstrap() {
    printf '  2/6 bootstrap pre-tools ordering and plugin pinning\n'
    write_bootstrap_config
    run_mise bootstrap --only tools --yes >/dev/null
    [[ "$(git -C "$MISE_DATA_DIR/plugins/skills" rev-parse HEAD)" == "$PLUGIN_REF_ONE" ]]
    grep -Fq "bootstrap-plugin install skills $PLUGIN_REF_ONE" "$LOG_FILE"
    grep -Fq '"event":"backend-install"' "$LOG_FILE"

    # A second bootstrap must inspect the existing URL/ref rather than reinstall.
    run_mise bootstrap --only tools --yes >/dev/null
    grep -Fq "bootstrap-plugin unchanged skills $PLUGIN_REF_ONE" "$LOG_FILE"

    # Changing the pinned ref replaces the plugin explicitly; --force is never used.
    bash "$SCRIPT_DIR/bootstrap-plugin.sh" skills "$PLUGIN_URL" "$PLUGIN_REF_TWO" >/dev/null
    [[ "$(git -C "$MISE_DATA_DIR/plugins/skills" rev-parse HEAD)" == "$PLUGIN_REF_TWO" ]]
    grep -Fq "bootstrap-plugin replace skills $PLUGIN_REF_ONE -> $PLUGIN_REF_TWO" "$LOG_FILE"
    ! grep -Fq -- '--force' "$SCRIPT_DIR/bootstrap-plugin.sh"
}

run_options() {
    printf '  3/6 string, array, and nested tool options\n'
    bash "$SCRIPT_DIR/bootstrap-plugin.sh" skills "$PLUGIN_URL" "$PLUGIN_REF_ONE" >/dev/null
    cat > mise.toml <<EOF
[plugins]
"vfox-backend:skills" = "$PLUGIN_URL#$PLUGIN_REF_ONE"

[tools]
"skills:string" = "0.0.0"
"skills:array" = ["0.0.0"]

[tools."skills:nested"]
version = "0.0.0"
source = "github:fullmeta-dev/skills"
include = ["tag:web", "code-review"]
exclude = ["tag:experimental"]
agents = ["pi", "codex"]

[tools."skills:nested".selection]
mode = "locked"
selectors = ["one", "two"]
EOF
    run_mise install --jobs=1 >/dev/null
    python3 - "$LOG_FILE" <<'PY'
import json
import sys

records = []
for line in open(sys.argv[1], encoding="utf-8"):
    if line.startswith("{"):
        records.append(json.loads(line))
by_tool = {record["tool"]: record for record in records}
assert {"string", "array", "nested"} <= by_tool.keys(), by_tool
assert by_tool["string"]["version"] == "0.0.0"
assert by_tool["array"]["version"] == "0.0.0"
assert by_tool["string"]["options"] == {}
assert by_tool["array"]["options"] == {}
assert by_tool["nested"]["options"] == {
    "source": "github:fullmeta-dev/skills",
    "include": ["tag:web", "code-review"],
    "exclude": ["tag:experimental"],
    "agents": ["pi", "codex"],
    "selection": {"mode": "locked", "selectors": ["one", "two"]},
}
PY
}

run_postinstall() {
    printf '  4/6 postinstall hook after a no-op install\n'
    bash "$SCRIPT_DIR/bootstrap-plugin.sh" skills "$PLUGIN_URL" "$PLUGIN_REF_ONE" >/dev/null

    local fixture_dir="$CASE_DIR/github-fixture"
    local port_file="$fixture_dir/port"
    local asset_file="$fixture_dir/mise-skills-fixture"
    local request_log="$fixture_dir/requests.log"
    mkdir -p "$fixture_dir"
    cat > "$asset_file" <<'EOF'
#!/bin/sh
printf '%s\n' cli >> "$SKILLS_COMPAT_LOG"
for arg in "$@"; do printf 'arg=%s\n' "$arg" >> "$SKILLS_COMPAT_LOG"; done
EOF
    chmod +x "$asset_file"
    : > "$request_log"
    cat > "$fixture_dir/server.py" <<'PY'
import http.server
import json
import sys
from urllib.parse import urlparse

port_file, asset_file, request_log = sys.argv[1:]


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
            with open(asset_file, "rb") as asset:
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
with open(port_file, "w", encoding="ascii") as port:
    port.write(str(server.server_address[1]))
server.serve_forever()
PY
    python3 -u "$fixture_dir/server.py" "$port_file" "$asset_file" \
        "$request_log" > "$fixture_dir/server.log" 2>&1 &
    GITHUB_FIXTURE_PID=$!
    for _ in {1..50}; do
        [[ -s "$port_file" ]] && break
        sleep 0.1
    done
    [[ -s "$port_file" ]]
    local github_api_url="http://127.0.0.1:$(cat "$port_file")"
    cat > "$MISE_CONFIG_DIR/config.toml" <<EOF
[plugins]
"vfox-backend:skills" = "$PLUGIN_URL#$PLUGIN_REF_ONE"

[tools]
"github:fullmeta-dev/skills" = { version = "0.1.0", bin = "mise-skills", asset_pattern = "mise-skills-fixture", api_url = "$github_api_url" }

[tasks.skills]
dir = "{{cwd}}"
run = "mise-skills \${usage_arguments-}"
raw = true
EOF
    cat > mise.toml <<'EOF'
[tools."skills:source"]
version = "0.0.0"

[hooks]
postinstall = ["mise run skills -- sync --locked"]
EOF
    run_mise install --jobs=1 >/dev/null
    run_mise install --jobs=1 >/dev/null
    [[ "$(grep -c '^cli$' "$LOG_FILE")" == 2 ]]
    grep -Fq '/repos/fullmeta-dev/skills/releases/tags/v0.1.0' "$request_log"
    grep -Fq '/assets/mise-skills-fixture' "$request_log"
    printf '  5/6 globally managed mise-skills is on the postinstall PATH\n'
    [[ "$(grep -c '^arg=sync$' "$LOG_FILE")" == 2 ]]
    [[ "$(grep -c '^arg=--locked$' "$LOG_FILE")" == 2 ]]
    printf '  6/6 explicit task invocation preserves argv\n'
    stop_github_fixture
}

run_selected_release() {
    local label="$1" binary="$2"
    printf '\n[%s] mise %s\n' "$label" "$(version_of "$binary")"
    begin_case "$label" "$binary"
    run_lifecycle
    begin_case "$label-bootstrap" "$binary"
    run_bootstrap
    begin_case "$label-options" "$binary"
    run_options
    begin_case "$label-postinstall" "$binary"
    run_postinstall
    printf '[%s] passed\n' "$label"
}

run_old_release_rejection() {
    local binary="$1" status output
    printf '\n[older-release-rejection] mise %s\n' "$(version_of "$binary")"
    begin_case older-release "$binary"
    write_bootstrap_config
    set +e
    output="$(run_mise bootstrap --yes 2>&1)"
    status=$?
    set -e
    # mise 2026.6.13 wraps the pre-tools hook's status 78 as bootstrap status 1.
    [[ "$status" == 1 ]]
    grep -Fq "requires mise >= $MINIMUM_VERSION" <<< "$output"
    grep -Fq 'Upgrade mise before installing skills' <<< "$output"
    [[ ! -e "$MISE_DATA_DIR/plugins/skills" ]]
    [[ ! -e "$MISE_DATA_DIR/installs" ]]
    [[ ! -s "$LOG_FILE" ]]
    printf '%s\n' "$output"
    printf '[older-release-rejection] passed\n'
}

ORIGINAL_PATH="$PATH"
make_plugin_repo

minimum_binary="$(resolve_binary "$MINIMUM_VERSION" "$MINIMUM_BINARY_OVERRIDE")"
latest_version_value="$(latest_version)"
latest_binary="$(resolve_binary "$latest_version_value" "$LATEST_BINARY_OVERRIDE")"
older_version="$(normalise_version "$OLDER_VERSION_OVERRIDE")"
older_binary="$(resolve_binary "$older_version" "$OLDER_BINARY_OVERRIDE")"

run_selected_release minimum "$minimum_binary"
run_selected_release latest "$latest_binary"
run_old_release_rejection "$older_binary"
printf '\nall Milestone 0 compatibility checks passed (minimum=%s, latest=%s)\n' \
    "$MINIMUM_VERSION" "$latest_version_value"
