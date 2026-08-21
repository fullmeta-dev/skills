#!/usr/bin/env bash
# Backend-only integration checks for immutable local, GitHub, and Git sources.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
PLUGIN_DIR="$REPO_ROOT/packages/skills-plugin"
FIXTURE_DIR="$SCRIPT_DIR/fixtures/backend-source"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mise-skills-backend.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

json_lua=''
read -r -d '' json_lua <<'LUA' || true
package.preload.file = function()
    return {
        join_path = function(...)
            local parts = {...}
            return table.concat(parts, "/")
        end,
    }
end
package.preload.json = function()
    local function quote(value)
        return '"' .. value:gsub('\\', '\\\\'):gsub('"', '\\"')
            :gsub('\n', '\\n'):gsub('\r', '\\r'):gsub('\t', '\\t') .. '"'
    end
    local function encode(value)
        if type(value) == "string" then return quote(value) end
        if type(value) == "number" or type(value) == "boolean" then return tostring(value) end
        if value == nil then return "null" end
        local keys = {}
        for key in pairs(value) do keys[#keys + 1] = key end
        table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
        local array = true
        for index, key in ipairs(keys) do
            if key ~= index then array = false; break end
        end
        local output = {}
        if array then
            for _, key in ipairs(keys) do output[#output + 1] = encode(value[key]) end
            return "[" .. table.concat(output, ",") .. "]"
        end
        for _, key in ipairs(keys) do
            output[#output + 1] = quote(tostring(key)) .. ":" .. encode(value[key])
        end
        return "{" .. table.concat(output, ",") .. "}"
    end
    return {encode = encode}
end

local backend = dofile(arg[1])
local mode = arg[2]
local tool = arg[3]
local source = arg[4]
local version = arg[5]
local destination = arg[6]
local config_root = arg[7]
local subpath = arg[8]
if mode == "list" then
    local ok, result = pcall(backend.list_versions, {tool = tool})
    if not ok then
        io.stderr:write(tostring(result), "\n")
        os.exit(1)
    end
    print(require("json").encode(result))
    os.exit(0)
end

local options = {
    source = source,
    config_root = config_root,
    source_subpath = subpath,
    track = "main",
}
local ok, result = pcall(backend.install, {
    tool = tool,
    version = version,
    install_path = destination,
    options = options,
})
if not ok then
    io.stderr:write(tostring(result), "\n")
    os.exit(1)
end
if mode == "install" then
    print("installed")
end
LUA

run_backend() {
    lua - "$PLUGIN_DIR/lib/skills_backend.lua" "$@" <<< "$json_lua"
}

source_root="$WORK_DIR/local-source"
cp -a "$FIXTURE_DIR" "$source_root"
config_root="$WORK_DIR/config-root"
mkdir -p "$config_root"
local_bad="tree-0000000000000000000000000000000000000000000000000000000000000000"
set +e
local_error="$(run_backend install local/local-source "path:../local-source" "$local_bad" "$WORK_DIR/local-bad" "$config_root" skills 2>&1)"
local_status=$?
set -e
[[ "$local_status" -ne 0 ]]
grep -Fq 'local source changed' <<< "$local_error"
legacy_error="$(run_backend install local/local-source "path:../local-source" "tree:0000000000000000000000000000000000000000000000000000000000000000" "$WORK_DIR/local-legacy" "$config_root" skills 2>&1 || true)"
grep -Fq 'local sources require version tree-' <<< "$legacy_error"
local_digest="$(sed -nE 's/.*found ([0-9a-f]{64}).*/\1/p' <<< "$local_error" | head -n1)"
[[ "$local_digest" =~ ^[0-9a-f]{64}$ ]]
local_version="tree-$local_digest"
run_backend install local/local-source "path:../local-source" "$local_version" "$WORK_DIR/local-install" "$config_root" skills >/dev/null
[[ -f "$WORK_DIR/local-install/skills/demo/SKILL.md" ]]
[[ -x "$WORK_DIR/local-install/skills/demo/scripts/run.sh" ]]
# The backend artifact is the complete source snapshot; selection is applied
# later by the lifecycle CLI.
grep -Fq '"subpath":"."' "$WORK_DIR/local-install/.mise-skills-manifest.json"
grep -Fq 'skills/demo' "$WORK_DIR/local-install/.mise-skills-manifest.json"
! grep -Fq 'track' "$WORK_DIR/local-install/.mise-skills-manifest.json"
touch "$source_root/skills/demo/SKILL.md"
run_backend install local/local-source "path:../local-source" "$local_version" "$WORK_DIR/local-mtime" "$config_root" skills >/dev/null
mkdir -p "$source_root/skills/.git"
printf '%s\n' backend-state > "$source_root/skills/.git/state"
run_backend install local/local-source "path:../local-source" "$local_version" "$WORK_DIR/local-git-state" "$config_root" skills >/dev/null

printf 'local source hashing and manifest: passed\n'
printf '%s\n' 'changed' >> "$source_root/skills/demo/SKILL.md"
set +e
changed_error="$(run_backend install local/local-source "path:../local-source" "$local_version" "$WORK_DIR/local-drift" "$config_root" skills 2>&1)"
changed_status=$?
set -e
[[ "$changed_status" -ne 0 ]]
grep -Fq 'local source changed' <<< "$changed_error"

set +e
unsafe_error="$(run_backend install local/local-source "path:../local-source" "$local_version" "$WORK_DIR/unsafe" "$config_root" ../outside 2>&1)"
unsafe_status=$?
set -e
[[ "$unsafe_status" -ne 0 ]]
grep -Fq 'source subpath' <<< "$unsafe_error"

outside="$WORK_DIR/outside.txt"
printf '%s\n' unsafe > "$outside"
ln -s "$outside" "$source_root/escaping-link"
set +e
link_error="$(run_backend install local/local-source "path:../local-source" "$local_version" "$WORK_DIR/link" "$config_root" skills 2>&1)"
link_status=$?
set -e
[[ "$link_status" -ne 0 ]]
grep -Fq 'unsafe source links' <<< "$link_error"

# Use one repository for both generic Git and GitHub fixtures. The first
# install exercises fetching an absent pinned commit into a fresh backend
# cache; the second succeeds after the remote is removed.
git_source="$WORK_DIR/git-source"
cp -a "$FIXTURE_DIR" "$git_source"
git -C "$git_source" init -q
git -C "$git_source" add .
git -C "$git_source" -c user.email=backend@example.invalid -c user.name=backend commit -qm fixture
revision="$(git -C "$git_source" rev-parse HEAD)"

list_output="$(run_backend list "git:file://$git_source" ignored ignored ignored ignored ignored ignored)"
grep -Fq "$revision" <<< "$list_output"

mkdir -p "$WORK_DIR/git-home" "$WORK_DIR/git-cache"
MISE_CACHE_DIR="$WORK_DIR/git-cache" run_backend install local/git-source "git:file://$git_source" "rev:$revision" "$WORK_DIR/git-install" "$config_root" skills >/dev/null
[[ -f "$WORK_DIR/git-install/skills/demo/SKILL.md" ]]
grep -Fq '"subpath":"."' "$WORK_DIR/git-install/.mise-skills-manifest.json"
grep -Fq "rev:$revision" "$WORK_DIR/git-install/.mise-skills-manifest.json"
# The same source revision can be installed for a different declaration
# subpath without changing the artifact contents or manifest identity.
MISE_CACHE_DIR="$WORK_DIR/git-cache" run_backend install local/git-source "git:file://$git_source" "rev:$revision" "$WORK_DIR/git-root-install" "$config_root" "" >/dev/null
[[ -f "$WORK_DIR/git-root-install/skills/demo/SKILL.md" ]]
cmp "$WORK_DIR/git-install/skills/demo/SKILL.md" "$WORK_DIR/git-root-install/skills/demo/SKILL.md"
# Native mise ls-remote cannot emit the custom rev: prefix. Its raw commit
# output must still be accepted by install and normalized in the manifest.
MISE_CACHE_DIR="$WORK_DIR/git-cache" run_backend install local/git-source "git:file://$git_source" "$revision" "$WORK_DIR/git-raw-version" "$config_root" skills >/dev/null
grep -Fq "rev:$revision" "$WORK_DIR/git-raw-version/.mise-skills-manifest.json"

mkdir -p "$WORK_DIR/github-base/owner"
git clone -q --bare "$git_source" "$WORK_DIR/github-base/owner/repo.git"
MISE_CACHE_DIR="$WORK_DIR/github-cache" MISE_SKILLS_GITHUB_BASE_URL="file://$WORK_DIR/github-base" \
    run_backend install owner/repo "github:owner/repo" "rev:$revision" "$WORK_DIR/github-install" "$config_root" skills >/dev/null
[[ -f "$WORK_DIR/github-install/skills/demo/SKILL.md" ]]
rm -rf "$WORK_DIR/github-base"
MISE_CACHE_DIR="$WORK_DIR/github-cache" MISE_SKILLS_GITHUB_BASE_URL="file://$WORK_DIR/github-base" \
    run_backend install owner/repo "github:owner/repo" "rev:$revision" "$WORK_DIR/github-cached" "$config_root" skills >/dev/null
[[ -f "$WORK_DIR/github-cached/skills/demo/SKILL.md" ]]

printf 'generic Git and GitHub exact-commit cache/fetch: passed\n'

set +e
key_error="$(run_backend install wrong/key "github:owner/repo" "rev:$revision" "$WORK_DIR/key" "$config_root" skills 2>&1)"
key_status=$?
set -e
[[ "$key_status" -ne 0 ]]
grep -Fq 'does not identify source' <<< "$key_error"

printf 'key, drift, traversal, and escaping-link rejection: passed\n'
