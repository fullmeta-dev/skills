local file = require("file")
local json = require("json")

local backend = {}

local function fail(message)
    error("skills backend: " .. message)
end

local function quote(value)
    if type(value) ~= "string" or value:find("%z") then
        fail("cannot use a NUL-containing value in a command")
    end
    return "'" .. value:gsub("'", "'\\''") .. "'"
end

local function execute(command)
    local output_path = os.tmpname()
    local status = os.execute(command .. " >" .. quote(output_path) .. " 2>&1")
    local output_file = io.open(output_path, "r")
    local output = output_file and output_file:read("*a") or ""
    if output_file ~= nil then
        output_file:close()
    end
    os.remove(output_path)
    return status, output
end

local function run(command)
    local status, output = execute(command)
    if status ~= true and status ~= 0 then
        local message = output:gsub("^%s+", ""):gsub("%s+$", "")
        fail(message ~= "" and message or ("command failed with status " .. tostring(status)))
    end
    return output
end

local function try_run(command)
    local status, output = execute(command)
    if status == true or status == 0 then
        return true, output
    end
    return false, output
end

local function digest_text(value)
    local input_path = os.tmpname()
    local input = io.open(input_path, "wb")
    if input == nil then
        fail("cannot create a temporary cache-key input")
    end
    input:write(value)
    input:close()
    local status, output = execute("(command -v sha256sum >/dev/null 2>&1 && sha256sum || shasum -a 256) < "
        .. quote(input_path))
    os.remove(input_path)
    if status ~= true and status ~= 0 then
        local message = output:gsub("^%s+", ""):gsub("%s+$", "")
        fail("cannot hash a cache key: " .. message)
    end
    local clean = output:gsub("^%s+", ""):gsub("%s+$", "")
    local digest = clean:match("^([0-9a-fA-F]+)")
    if digest == nil then
        fail("cache-key hash command returned no digest")
    end
    return digest
end

local function trim(value)
    return (value:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function split_lines(value)
    local result = {}
    for line in value:gmatch("[^\r\n]+") do
        result[#result + 1] = line
    end
    return result
end

local function path_join(left, right)
    if right == "" then
        return left
    end
    return file.join_path(left, right)
end

local function path_basename(path)
    local clean = path:gsub("[\\/]$", "")
    return clean:match("([^\\/]+)$") or clean
end

local function path_dirname(path)
    local clean = path:gsub("[\\/]$", "")
    local parent = clean:match("^(.*)[\\/][^\\/]+$")
    return parent or "."
end

local function is_absolute(path)
    return path:sub(1, 1) == "/" or path:match("^[A-Za-z]:[\\/]") ~= nil
end

local function normalise_subpath(value)
    if value == nil or value == "" then
        return ""
    end
    if type(value) ~= "string" then
        fail("source subpath must be a string")
    end
    if value:find("%z") or value:find("\\") or is_absolute(value) then
        fail("unsafe source subpath " .. quote(value))
    end

    local parts = {}
    for part in value:gmatch("[^/]+") do
        if part == ".." then
            fail("source subpath may not contain '..': " .. value)
        elseif part ~= "." then
            parts[#parts + 1] = part
        end
    end
    return table.concat(parts, "/")
end

local function source_options(options)
    if type(options) ~= "table" then
        fail("a source option is required for skills tools")
    end
    local source = options.source
    if type(source) ~= "string" or source == "" then
        fail("source must be one of path:, github:, or git:")
    end

    local subpath = options.source_subpath
    if subpath == nil then
        subpath = options.subpath
    end
    if subpath == nil then
        subpath = options.source_path
    end
    if options.source_subpath ~= nil and options.subpath ~= nil
        and options.source_subpath ~= options.subpath then
        fail("source_subpath and subpath disagree")
    end
    return source, normalise_subpath(subpath)
end

local function config_root(options)
    local candidate
    if type(options) == "table" then
        candidate = options.config_root or options.declaring_config_root
        if candidate == nil and options.config_path ~= nil then
            candidate = path_dirname(options.config_path)
        end
        if candidate == nil and options.declaring_config ~= nil then
            candidate = path_dirname(options.declaring_config)
        end
    end
    candidate = candidate or os.getenv("MISE_CONFIG_ROOT")
    if candidate == nil or candidate == "" then
        fail("relative path sources require the declaring config root")
    end
    local absolute = is_absolute(candidate) and candidate or path_join(trim(run("pwd")), candidate)
    local real, output = try_run("realpath " .. quote(absolute))
    if real then
        return trim(output)
    end
    return absolute
end

local function realpath(path)
    local ok, output = try_run("realpath " .. quote(path))
    if not ok then
        fail("source path does not exist: " .. path)
    end
    return trim(output)
end

local function source_kind(source)
    if source:sub(1, 5) == "path:" then
        local value = source:sub(6)
        if value == "" then
            fail("path: source cannot be empty")
        end
        return "path", value
    end
    if source:sub(1, 7) == "github:" then
        local value = source:sub(8)
        if not value:match("^[%w_.-]+/[%w_.-]+$")
            or value:find("%.%.") then
            fail("invalid GitHub source " .. source)
        end
        return "github", value
    end
    if source:sub(1, 4) == "git:" then
        local value = source:sub(5)
        if value == "" or value:find("[%z\r\n]") or value:match("^%-") then
            fail("invalid Git source " .. source)
        end
        if not value:match("^[%w+.-]+://")
            and not value:match("^[%w_.-]+@[^:]+:.+") then
            fail("Git source must be a URL or SCP-style SSH address: " .. source)
        end
        if value:find("#") then
            fail("Git source must not contain a ref fragment: " .. source)
        end
        return "git", value
    end
    fail("source must use path:, github:, or git: (got " .. source .. ")")
end

local function canonical_git_key(value)
    local key = value:gsub("^[%w+.-]+://", "")
    key = key:gsub("^[^@/]+@", "")
    key = key:gsub("^([^:]+):", "%1/")
    key = key:gsub("/$", ""):gsub("%.git$", "")
    return key
end

local function expected_tool_keys(source, kind, value, options)
    if type(options) == "table" then
        local explicit = options.source_id or options.source_key or options.key
        if explicit ~= nil then
            if type(explicit) ~= "string" or explicit == "" then
                fail("source_id must be a non-empty string")
            end
            return {explicit}
        end
    end

    if kind == "github" then
        return {value}
    elseif kind == "git" then
        local key = canonical_git_key(value)
        local keys = {key}
        -- File URLs are useful for isolated integration fixtures and have no
        -- host/repository identity. Keep their generated key consistent with
        -- path sources while retaining the full canonical key for real Git
        -- URLs.
        if value:match("^file://") then
            local file_path = value:gsub("^file://", "")
            local basename = path_basename(file_path)
            keys[#keys + 1] = "local/" .. basename
            keys[#keys + 1] = basename
        end
        return keys
    end

    local basename = path_basename(value)
    return {"local/" .. basename, basename, value}
end

local function normalise_tool(tool)
    if type(tool) ~= "string" or tool == "" then
        fail("tool key cannot be empty")
    end
    if tool:sub(1, 7) == "skills:" then
        return tool:sub(8)
    end
    return tool
end

local function validate_tool_key(tool, source, kind, value, options)
    local actual = normalise_tool(tool)
    local keys = expected_tool_keys(source, kind, value, options)
    for _, expected in ipairs(keys) do
        if actual == expected then
            return actual
        end
    end
    fail("tool key '" .. actual .. "' does not identify source '" .. source .. "'")
end

local function validate_version(kind, version)
    if type(version) ~= "string" then
        fail("version must be an immutable tree-<sha256> or rev:<full-commit>")
    end
    if kind == "path" then
        local digest = version:match("^tree%-([0-9a-fA-F]+)$")
        if digest == nil or #digest ~= 64 then
            fail("local sources require version tree-<64 hexadecimal characters>")
        end
        return "tree", digest:lower(), "tree-" .. digest:lower()
    end

    local revision = version:match("^rev:([0-9a-fA-F]+)$")
    if revision == nil then
        -- mise's native `ls-remote` surface accepts ordinary versions but
        -- rejects the custom rev: prefix. Keep the list output usable as an
        -- install input while recording the immutable form canonically.
        revision = version:match("^([0-9a-fA-F]+)$")
    end
    if revision == nil or #revision ~= 40 then
        fail("Git sources require version rev:<full 40-character commit> or a raw full commit")
    end
    return "rev", revision:lower(), "rev:" .. revision:lower()
end

local function validate_links(root)
    local script = [[
set -eu
set -o pipefail
export LC_ALL=C
root=$1
root_real=$(realpath "$root")
find "$root" \( -name .git -o -name .mise-skills-manifest.json -o -name .skills-backend \) -prune -o \
    -type l -print0 | while IFS= read -r -d '' link; do
    target=$(readlink "$link")
    if [ -z "$target" ]; then
        echo "empty symbolic link: $link" >&2
        exit 1
    fi
    link_dir=$(dirname "$link")
    resolved=$(cd "$link_dir" && realpath "$target") || {
        echo "broken symbolic link: $link" >&2
        exit 1
    }
    case "$resolved" in
        "$root_real"|"$root_real"/*) ;;
        *) echo "symbolic link escapes source root: $link -> $target" >&2; exit 1 ;;
    esac
done
]]
    local command = "bash -c " .. quote(script .. "\n") .. " -- " .. quote(root)
    local ok, output = try_run(command)
    if not ok then
        fail("unsafe source links in " .. root .. ": " .. output)
    end
end

local function tree_digest(root)
    validate_links(root)
    local script = [[
set -eu
set -o pipefail
export LC_ALL=C
root=$1
find "$root" -mindepth 1 \( -name .git -o -name .mise-skills-manifest.json -o -name .skills-backend \) -prune -o -print0 |
    sort -z |
    while IFS= read -r -d '' path; do
        rel=${path#"$root"/}
        if [ -L "$path" ]; then
            type=symlink
            executable=0
            payload_file=$(mktemp)
            readlink "$path" > "$payload_file"
            length=$(wc -c < "$payload_file" | tr -d ' ')
            printf '%s\0%s\0%s\0%s\0' "$rel" "$type" "$executable" "$length"
            cat "$payload_file"
            rm -f "$payload_file"
        elif [ -d "$path" ]; then
            type=directory
            permissions=$(stat -c '%A' "$path" 2>/dev/null || stat -f '%Sp' "$path")
            case "$permissions" in *x*) executable=1;; *) executable=0;; esac
            printf '%s\0%s\0%s\0%s\0' "$rel" "$type" "$executable" 0
        elif [ -f "$path" ]; then
            type=file
            permissions=$(stat -c '%A' "$path" 2>/dev/null || stat -f '%Sp' "$path")
            case "$permissions" in *x*) executable=1;; *) executable=0;; esac
            length=$(wc -c < "$path" | tr -d ' ')
            printf '%s\0%s\0%s\0%s\0' "$rel" "$type" "$executable" "$length"
            cat "$path"
        else
            type=other
            permissions=$(stat -c '%A' "$path" 2>/dev/null || stat -f '%Sp' "$path")
            case "$permissions" in *x*) executable=1;; *) executable=0;; esac
            printf '%s\0%s\0%s\00\0' "$rel" "$type" "$executable"
        fi
        printf '\0'
    done |
    (command -v sha256sum >/dev/null 2>&1 && sha256sum || shasum -a 256) |
    awk '{print $1}'
]]
    local command = "bash -c " .. quote(script .. "\n") .. " -- " .. quote(root)
    return trim(run(command))
end

local function make_temp_dir(prefix)
    local template = os.getenv("TMPDIR") or "/tmp"
    local output = run("mktemp -d " .. quote(path_join(template, prefix .. ".XXXXXX")))
    return trim(output)
end

local function ensure_directory(path)
    run("mkdir -p " .. quote(path))
end

local function copy_snapshot(source_root, destination)
    ensure_directory(destination)
    run("cp -a " .. quote(path_join(source_root, ".")) .. " " .. quote(destination))
    run("find " .. quote(destination) .. " -mindepth 1 "
        .. "\\( -name .git -o -name .mise-skills-manifest.json "
        .. "-o -name .skills-backend \\) -prune -exec rm -rf -- {} \\;")
end

local function source_subroot(root, subpath)
    local selected = subpath == "" and root or path_join(root, subpath)
    local selected_real = realpath(selected)
    local root_real = realpath(root)
    if selected_real ~= root_real and selected_real:sub(1, #root_real + 1) ~= root_real .. "/" then
        fail("source subpath escapes source root: " .. subpath)
    end
    local ok, output = try_run("test -d " .. quote(selected_real))
    if not ok then
        fail("source subpath is not a directory: " .. subpath)
    end
    return selected_real
end

local function git_source_url(kind, value)
    if kind == "github" then
        local base = os.getenv("MISE_SKILLS_GITHUB_BASE_URL") or "https://github.com"
        return base:gsub("/$", "") .. "/" .. value .. ".git"
    end
    return value
end

local function git_snapshot(kind, value, revision)
    local url = git_source_url(kind, value)
    local cache_base = os.getenv("MISE_CACHE_DIR") or path_join(os.getenv("HOME") or "/tmp", ".cache/mise")
    local cache_name = (url .. "-" .. revision):gsub("[^A-Za-z0-9._-]", "_")
    local cache = path_join(path_join(cache_base, "skills-backend"), cache_name)
    ensure_directory(path_dirname(cache))

    local is_repo = try_run("test -d " .. quote(path_join(cache, ".git")))
    if not is_repo then
        run("rm -rf " .. quote(cache))
        ensure_directory(cache)
        run("git -C " .. quote(cache) .. " init -q")
        run("git -C " .. quote(cache) .. " remote add origin " .. quote(url))
    end

    local present = try_run("git -C " .. quote(cache) .. " cat-file -e " .. quote(revision .. "^{commit}"))
    if not present then
        local fetched, output = try_run("git -C " .. quote(cache)
            .. " fetch --no-tags origin " .. quote(revision))
        if not fetched then
            fail("unable to fetch pinned commit " .. revision .. " from " .. url .. ": " .. output)
        end
    end
    local verified, output = try_run("git -C " .. quote(cache) .. " rev-parse --verify "
        .. quote(revision .. "^{commit}"))
    if not verified or trim(output):lower() ~= revision:lower() then
        fail("Git source did not provide requested commit " .. revision)
    end

    local checkout = make_temp_dir("mise-skills-git")
    local archived, archive_output = try_run("git -C " .. quote(cache) .. " archive --format=tar "
        .. quote(revision) .. " | tar -xf - -C " .. quote(checkout))
    if not archived then
        fail("unable to materialize pinned commit " .. revision .. ": " .. archive_output)
    end
    return checkout
end

local function discover_skills(root)
    local output = run("find " .. quote(root)
        .. " -type d \\( -name .git -o -name node_modules -o -name dist "
        .. "-o -name build -o -name __pycache__ -o -name .mise-skills-manifest.json "
        .. "-o -name .skills-backend \\) -prune -o -type f -name SKILL.md -print | sort")
    local skills = {}
    for _, path in ipairs(split_lines(output)) do
        local directory = path_dirname(path)
        local relative = directory
        if directory == root then
            relative = "."
        elseif directory:sub(1, #root + 1) == root .. "/" then
            relative = directory:sub(#root + 2)
        end
        skills[#skills + 1] = {
            path = relative,
            tree = tree_digest(directory),
        }
    end
    return skills
end

local function write_manifest(destination, source, subpath, version, revision, snapshot_digest)
    local manifest = {
        schema_version = 1,
        source = source,
        subpath = subpath == "" and "." or subpath,
        version = version,
        revision = revision,
        tree = snapshot_digest,
        skills = discover_skills(destination),
    }
    local path = path_join(destination, ".mise-skills-manifest.json")
    local output = io.open(path, "w")
    if output == nil then
        fail("cannot write artifact manifest: " .. path)
    end
    output:write(json.encode(manifest))
    output:write("\n")
    output:close()
end

function backend.install(ctx)
    if type(ctx) ~= "table" then
        fail("backend install context is missing")
    end
    local source, subpath = source_options(ctx.options)
    local kind, value = source_kind(source)
    validate_tool_key(ctx.tool, source, kind, value, ctx.options)
    local version_kind, version_value, canonical_version = validate_version(kind, ctx.version)

    local source_root
    local temporary_root
    if kind == "path" then
        local configured_path = value
        local root = is_absolute(configured_path) and configured_path
            or path_join(config_root(ctx.options), configured_path)
        source_root = realpath(root)
        local is_directory = try_run("test -d " .. quote(source_root))
        if not is_directory then
            fail("path source is not a directory: " .. configured_path)
        end
    else
        source_root = git_snapshot(kind, value, version_value)
        temporary_root = source_root
    end

    validate_links(source_root)
    -- Validate the declaration's selection even though it must not narrow the
    -- immutable artifact copied below.
    source_subroot(source_root, subpath)
    -- Install one complete, source+version keyed snapshot.  The source
    -- subpath is a lifecycle selection, not an artifact boundary: two
    -- declarations at the same revision must be able to reuse this exact
    -- installation without whichever declaration ran first changing its
    -- contents.
    local expected_digest = tree_digest(source_root)
    if version_kind == "tree" and expected_digest:lower() ~= version_value then
        fail("local source changed: declared " .. version_value .. " but found " .. expected_digest)
    end

    local canonical_source = source
    if kind == "path" then
        canonical_source = "path:" .. source_root
    end

    local destination = ctx.install_path
    if type(destination) ~= "string" or destination == "" then
        fail("mise did not supply an installation path")
    end
    run("rm -rf " .. quote(destination))
    copy_snapshot(source_root, destination)
    local installed_digest = tree_digest(destination)
    if installed_digest:lower() ~= expected_digest:lower() then
        fail("installed snapshot failed integrity verification")
    end
    -- The artifact itself is always rooted at the complete source snapshot.
    -- Keep this explicit in the manifest so lifecycle code can reject older
    -- source_subpath-only artifacts rather than activating the wrong tree.
    write_manifest(destination, canonical_source, "", canonical_version,
        kind == "path" and canonical_version or ("rev:" .. version_value), expected_digest)
    if temporary_root ~= nil then
        run("rm -rf " .. quote(temporary_root))
    end
    return {}
end

function backend.list_versions(ctx)
    local tool = normalise_tool(ctx and ctx.tool or "")
    if tool == "" then
        fail("tool key cannot be empty")
    end

    local source
    if tool:sub(1, 6) == "local/" then
        -- A local path source has no remote versions to list. More
        -- importantly, returning an empty list prevents mise from treating
        -- the generated local source key as GitHub shorthand before it calls
        -- BackendInstall with the already-resolved tree- pin.
        return {versions = {}}
    elseif tool:match("^[%w_.-]+/[%w_.-]+$") then
        source = "github:" .. tool
    elseif tool:sub(1, 7) == "github:" then
        source = tool
    elseif tool:sub(1, 4) == "git:" then
        source = tool
    else
        return {versions = {}}
    end

    local kind, value = source_kind(source)
    local url = git_source_url(kind, value)
    local ok, output = try_run("git ls-remote --heads --tags " .. quote(url))
    if not ok then
        fail("unable to list immutable commits for " .. source .. ": " .. output)
    end
    local revisions = {}
    local seen = {}
    for _, line in ipairs(split_lines(output)) do
        local sha = line:match("^([0-9a-fA-F]+)%s")
        if sha ~= nil and #sha == 40 and not seen[sha:lower()] then
            seen[sha:lower()] = true
            -- mise validates BackendListVersions values as ordinary release
            -- versions and rejects the custom rev: prefix there. BackendInstall
            -- accepts the raw commit too and canonicalizes it to rev:<commit>.
            revisions[#revisions + 1] = sha:lower()
        end
    end
    table.sort(revisions)
    return {versions = revisions}
end

return backend
