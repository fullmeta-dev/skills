# `skills` lifecycle design

This repository publishes the CLI as the public `skills` executable. Mise
manages that executable as an ordinary GitHub release tool, while the pinned
`vfox-backend:skills` plugin installs complete immutable source snapshots.
There is no task or wrapper command in the supported lifecycle.

## Lifecycle contract

1. Install the plugin at the reviewed URL and commit.
2. Declare the CLI with `bin = "skills"` and a concrete release version.
3. Run `mise install` for ordinary mise installation.
4. Run `skills use SOURCE ... --agent AGENT` to write an immutable source
   declaration.
5. Run `skills install` to invoke normal mise install for the selected config
   and then direct `skills sync --locked`.

`skills use` resolves GitHub/Git refs to `rev:<full-commit>` and local trees to
`tree-<sha256>`. `track` is update intent only. `skills upgrade` is the sole
source operation that follows a track; install, sync, and generated
postinstall hooks preserve the declared pin.

## Safe target selection

Project, global, explicit path, and mise environment targets are represented by
the CLI's `Scope` seam. `--env NAME` selects mise's environment-specific config
(`mise.NAME.toml` or `config.NAME.toml`), validates the name, and is passed to
normal mise as the documented environment context. Normal install receives the
selected config through `MISE_CONFIG_FILE` and never receives an arbitrary user
selector as a positional tool argument.

The effective config path and environment are printed by status and doctor.
The CLI does not merge project and global ownership manifests. Each config root
records only links it created under `$MISE_STATE_DIR/skills`.

## Collision and update safety

`skills install` checks both the PATH owner and `mise which skills` without
assuming undocumented precedence. A foreign executable blocks installation
unless `--accept-collision` is explicit. Skill-link collisions are reported by
sync and leave unmanaged files, directories, and symlinks untouched.

`skills self-update` discovers the latest GitHub release through its API and
reads that release's immutable `release-compatibility.json` asset. It checks the
minimum mise version, installs the requested CLI release, updates the exact
backend plugin pin, and commits the config only after the pair succeeds. The
config remains unchanged on a failed download or install; a failed plugin
replacement attempts to restore the old pin before reporting the error. Local
manifest/API paths are injectable for no-network tests.

## Release contract

`mise run release -- --patch|--minor|--major|--version VERSION` is the sole
operator entrypoint. It requires a clean, current `main`, validates Cargo
semver and tag availability before writing, then separately confirms the
version commit/tag push and external workflow dispatch. `--yes` accepts both;
`--resume --version VERSION` verifies and dispatches an already pushed matching
tag without changing Git. A failed dispatch retains the immutable tag.

The compatibility manifest pairs a CLI release with `minimum_mise`,
`plugin_url`, and the tagged commit's immutable `plugin_revision`. The manual
`workflow_dispatch` workflow must run at that tag, derives/verifies Cargo's
version, builds only Linux x86_64 and macOS arm64, emits SHA256 checksums, and
publishes the manifest and archives. Ordinary pushes and merges do not trigger
release builds. The manifest is generated as a release asset, not committed
or read from mutable `main`.
