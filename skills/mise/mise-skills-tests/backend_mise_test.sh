#!/usr/bin/env bash
# Isolated integration checks through a real mise process and the shipped plugin.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_DIR="$(cd -- "$SCRIPT_DIR/../plugin" && pwd)"
FIXTURE_DIR="$SCRIPT_DIR/fixtures/backend-source"
MISE_BIN="${MISE_BIN:-mise}"

if ! command -v "$MISE_BIN" >/dev/null 2>&1; then
    printf 'mise is required; set MISE_BIN to an executable mise binary\n' >&2
    exit 2
fi
if ! command -v git >/dev/null 2>&1; then
    printf 'git is required\n' >&2
    exit 2
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mise-skills-real-backend.XXXXXX")"
KEEP_TEMP="${MISE_SKILLS_KEEP_TEMP:-}"
cleanup() {
    if [[ "$KEEP_TEMP" == 1 ]]; then
        printf 'preserving real mise fixtures at %s\n' "$WORK_DIR"
    else
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

PLUGIN_REPO="$WORK_DIR/plugin"
LOCAL_SOURCE="$WORK_DIR/local-source"
GIT_SOURCE="$WORK_DIR/git-source"
GITHUB_BASE="$WORK_DIR/github"
HOME_DIR="$WORK_DIR/home"
CONFIG_DIR="$WORK_DIR/config"
DATA_DIR="$WORK_DIR/data"
STATE_DIR="$WORK_DIR/state"
CACHE_DIR="$WORK_DIR/cache"
PROJECT_DIR="$WORK_DIR/project"

mkdir -p "$PLUGIN_REPO" "$HOME_DIR" "$CONFIG_DIR" "$DATA_DIR" "$STATE_DIR" \
    "$CACHE_DIR" "$PROJECT_DIR" "$GITHUB_BASE/owner"
cp -a "$PLUGIN_DIR/." "$PLUGIN_REPO/"
git -C "$PLUGIN_REPO" init -q
git -C "$PLUGIN_REPO" add .
git -C "$PLUGIN_REPO" -c user.email=compat@example.invalid \
    -c user.name=compat commit -qm shipped-plugin
PLUGIN_REVISION="$(git -C "$PLUGIN_REPO" rev-parse HEAD)"

cp -a "$FIXTURE_DIR" "$LOCAL_SOURCE"
cp -a "$FIXTURE_DIR" "$GIT_SOURCE"
git -C "$GIT_SOURCE" init -q
git -C "$GIT_SOURCE" add .
git -C "$GIT_SOURCE" -c user.email=compat@example.invalid \
    -c user.name=compat commit -qm source-fixture
SOURCE_REVISION="$(git -C "$GIT_SOURCE" rev-parse HEAD)"
git clone -q --bare "$GIT_SOURCE" "$GITHUB_BASE/owner/repo.git"

export HOME="$HOME_DIR"
export MISE_CONFIG_DIR="$CONFIG_DIR"
export MISE_DATA_DIR="$DATA_DIR"
export MISE_STATE_DIR="$STATE_DIR"
export MISE_CACHE_DIR="$CACHE_DIR"
export MISE_TRUSTED_CONFIG_PATHS="$PROJECT_DIR:$CONFIG_DIR"
export MISE_SKILLS_GITHUB_BASE_URL="file://$GITHUB_BASE"
cd "$PROJECT_DIR"

mise_cmd() {
    "$MISE_BIN" "$@"
}

# This is the checked-in fixture's expected tree digest. It is intentionally a
# test oracle, not a documented user workflow for calculating local versions.
LOCAL_TREE_DIGEST="539d72aae084df5ce20ed5c4009aa88e4cf785934f45e48c3a39be2f18bd3a27"
LOCAL_VERSION="tree-$LOCAL_TREE_DIGEST"

printf 'installing shipped plugin through mise plugins install\n'
mise_cmd plugins install skills "file://$PLUGIN_REPO#$PLUGIN_REVISION" >/dev/null
[[ -f "$DATA_DIR/plugins/skills/hooks/backend_list_versions.lua" ]]
[[ -f "$DATA_DIR/plugins/skills/hooks/backend_install.lua" ]]

printf 'listing GitHub fixture revisions through mise ls-remote\n'
remote_versions="$(mise_cmd ls-remote skills:owner/repo)"
grep -Fxq "$SOURCE_REVISION" <<< "$remote_versions"

cat > mise.toml <<EOF
[tools."skills:owner/repo"]
source = "github:owner/repo"
version = "$SOURCE_REVISION"
source_subpath = "skills"
EOF

printf 'installing GitHub fixture through mise install\n'
mise_cmd install --jobs=1 > "$WORK_DIR/github-install.log" 2>&1 || {
    cat "$WORK_DIR/github-install.log" >&2
    exit 1
}
GITHUB_INSTALL="$DATA_DIR/installs/skills-owner-repo/$SOURCE_REVISION"
[[ -f "$GITHUB_INSTALL/skills/demo/SKILL.md" ]]
[[ -x "$GITHUB_INSTALL/skills/demo/scripts/run.sh" ]]
python3 - "$GITHUB_INSTALL/.mise-skills-manifest.json" "$SOURCE_REVISION" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
revision = sys.argv[2]
assert manifest["source"] == "github:owner/repo", manifest
assert manifest["subpath"] == ".", manifest
assert manifest["version"] == f"rev:{revision}", manifest
assert manifest["revision"] == f"rev:{revision}", manifest
assert manifest["skills"] == [{
    "path": "skills/demo",
    "tree": "5afb149df6000f52f46fdccc98d6f0888d9e54f46a01b1f695bf660868d0a49b",
}], manifest
PY

cat >> mise.toml <<EOF

[tools."skills:local-source"]
source = "path:$LOCAL_SOURCE"
version = "$LOCAL_VERSION"
source_subpath = "skills"
EOF

printf 'checking local fixture version listing through mise ls-remote\n'
mise_cmd ls-remote skills:local-source > "$WORK_DIR/local-ls-remote.log" 2>&1
printf 'installing local fixture through mise install\n'
mise_cmd install --jobs=1 > "$WORK_DIR/local-install.log" 2>&1 || {
    cat "$WORK_DIR/local-install.log" >&2
    exit 1
}
LOCAL_INSTALL="$DATA_DIR/installs/skills-local-source/$LOCAL_VERSION"
[[ -f "$LOCAL_INSTALL/skills/demo/SKILL.md" ]]
[[ -x "$LOCAL_INSTALL/skills/demo/scripts/run.sh" ]]
python3 - "$LOCAL_INSTALL/.mise-skills-manifest.json" "$LOCAL_VERSION" "$LOCAL_TREE_DIGEST" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
version, digest = sys.argv[2:]
assert manifest["source"].startswith("path:"), manifest
assert manifest["subpath"] == ".", manifest
assert manifest["version"] == version, manifest
assert manifest["revision"] == version, manifest
assert manifest["tree"] == digest, manifest
assert manifest["skills"][0]["path"] == "skills/demo", manifest
assert not any(
    value.startswith("tree:") for value in manifest.values() if isinstance(value, str)
), manifest
PY

printf 'real mise backend integration passed (plugin=%s source=%s)\n' \
    "$PLUGIN_REVISION" "$SOURCE_REVISION"
