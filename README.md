# Agent skills

This repository is an Agent Skills source tree and a mise backend/CLI. The
public executable is `skills`; it installs immutable source snapshots through
mise and materializes selected skills for supported agents.

## Quick start

Prerequisites: [mise](https://mise.jdx.dev/) 2026.11.7 or newer, Git, and a
supported agent. Release assets currently support Linux x86_64 and macOS
arm64 only. Install the backend plugin and the released CLI as ordinary
mise tools (no wrapper task is required):

```sh
mise plugins install skills https://github.com/fullmeta-dev/skills.git#<compatible-plugin-commit>
mise use --global --pin github:fullmeta-dev/skills@<release>
```

Set the CLI tool options in the global mise config:

```toml
[tools."github:fullmeta-dev/skills"]
version = "<release>"
bin = "skills"
```

For a private skills repository, mise cannot use the public versions host and
`self-update` needs a token for the release API and assets:

```sh
export GITHUB_TOKEN=<token>   # or MISE_GITHUB_TOKEN / GH_TOKEN
```

```toml
[settings]
use_versions_host = false
```

The backend plugin and skill sources are cloned with Git. If HTTPS Git
credentials are unavailable, use an SSH URL for those
(`git@github.com:owner/repository.git`); `github:` sources and the CLI release
install use HTTPS and the token.

Mise shell activation is required for the global binary to be on `PATH`. Then
install it through normal mise and select skills in a project. Browse a source
first; `list` is read-only and shows names, descriptions, tags, aliases, files,
and selector decisions:

```sh
skills list fullmeta-dev/skills --json --explain
skills use fullmeta-dev/skills mise --agent pi
skills install
skills status
```

`list` also accepts `path:DIR`, local directories, `github:owner/repository`,
and Git URLs (`https://`, `ssh://`, or `file://`). Remote sources are fetched
at an immutable revision into a read-only checkout. Use `--track BRANCH_OR_TAG`
or `--reference REF` to preview a particular revision without editing config.

`list` caches remote checkouts by revision (a revision cannot change) under
`$SKILLS_CACHE_DIR` (else `$MISE_CACHE_DIR/skills`, else `~/.cache/skills`),
and caches resolved refs for `SKILLS_CACHE_TTL` seconds (default 300). Repeat
listings of the same revision are then local. `--refresh` re-resolves refs
instead of using the cached resolution, `SKILLS_NO_CACHE=1` disables caching,
and `skills cache` reports or cleans the cache.

If `skills` is not the expected executable, compare `mise which skills` and
`command -v skills`. `skills doctor --global` reports a foreign executable;
remove or reorder that program explicitly after review. The installer never
deletes a pre-existing `skills` executable.

`skills use` resolves a full Git commit or local tree digest and writes the
selected config. A `git:` source is keyed by host and repository
(`git:git@github.com:owner/repo.git` becomes `skills:github.com/owner/repo`).
Installed artifacts are always complete source snapshots so two declarations
at the same revision share one immutable install; `source_subpath` and
`include`/`exclude` are applied at selection time, not baked into the artifact.
`skills install` coordinates normal `mise install` for that
config, then runs a locked link sync. The generated postinstall hook is the
direct command `skills sync --locked` for a project; a global or `--path`
declaration writes the matching `--global`/`--path` flags so the hook targets
its own config rather than the project mise happens to run it in.

Use `--global`, `--path PATH`, and `--env ENV` to select the intended mise
configuration. Project selections belong in `mise.toml`; environment-specific
profiles use `mise.<env>.toml`, while local-only choices can use
`mise.local.toml` or `mise.<env>.local.toml` (normally gitignored). The effective config is never inferred
from an arbitrary tool argument. `agents`, `codex`, and `cursor` use
`.agents/skills` (or `~/.agents/skills` globally); `pi` and `claude` use their
native locations. Generated links point into mise's installs and are
machine-specific, so a project sync records them in `.git/info/exclude` and
they do not appear in `git status` (a linked worktree is left alone because it
shares `info/exclude`). See the illustrative generated-style config
at
[`examples/skills-project.mise.toml`](examples/skills-project.mise.toml). For
other normal mise config scopes:

```sh
skills use --global fullmeta-dev/skills mise --agent agents
skills use --env development fullmeta-dev/skills mise --agent pi
skills use --path mise.local.toml fullmeta-dev/skills mise --agent pi
```

A source repository may add `.mise/skills.toml` to declare discovery roots,
tags, aliases, and exclusions; this repository does so to publish only
`skills/` and keep implementation/tests out of its catalog.

## Updates and removal

Source pins never move during install or sync. Refresh them explicitly, then
install the new immutable artifact:

```sh
skills upgrade
skills install
skills sync --locked
```

`use` records the remote default branch as the update track, so
`skills upgrade` is how "latest" is expressed; there is deliberately no
`version = "latest"`, because mise versions must be immutable. Sync and
install never overwrite an unmanaged or changed target. To adopt existing
files or symlinks (for example, when migrating from another skills manager),
pass `--force`: conflicting symlinks are removed and conflicting files or
directories are moved aside to `.skills-replaced-<name>-<timestamp>`.

Update the CLI and its pinned backend as a compatible pair:

```sh
skills self-update
```

`self-update` discovers the latest GitHub release through the release API and
reads its immutable `release-compatibility.json` asset. It inspects the
installed backend in mise plugin data, installs the release before changing the
selected config, and retains the old config when any step fails. Use
`skills self-update --dry-run` to print the pair and config plan without edits.
The backend plugin does not need a manually repeated `[plugins]` declaration.
For a no-network test or controlled fixture, set `SKILLS_RELEASE_API` (API
JSON) or `SKILLS_COMPATIBILITY_MANIFEST` (manifest JSON) to a local file.
Inspect
collisions before accepting a replacement:

```sh
skills doctor --global
skills install --accept-collision
```

Remove policy and owned links without touching unrelated targets:

```sh
skills remove skills:fullmeta-dev/skills mise
skills remove skills:fullmeta-dev/skills
skills prune
```

## Help and completions

```sh
skills --help
skills completions bash > ~/.local/share/bash-completion/completions/skills
skills completions zsh > ~/.zfunc/_skills
skills completions fish > ~/.config/fish/completions/skills.fish
skills completions powershell > "$HOME/.config/powershell/skills.ps1"
```

The concise reference is [`docs/mise-skills.md`](docs/mise-skills.md).

## Development

The backend implementation is under `packages/skills-plugin`; the Rust CLI is under
`packages/skills`.

For local product development, link the working-tree backend once and build the
local CLI. The repository mise configuration prepends the debug artifact to
`PATH`, so it shadows a released global `skills` executable only while working
in this checkout:

```sh
mise run link
mise run build
mise watch build # optional Rust rebuild loop
```

`mise run link` intentionally only links mise's global `skills` plugin; it does
not build the CLI. Lua/backend edits are visible through that link immediately.
After a release exists, return the global plugin and CLI to the latest compatible
released pair by running `skills self-update` outside this checkout.

Run the checks with:

```sh
cargo test --manifest-path packages/skills/Cargo.toml
tests/skills/backend_test.sh
tests/skills/e2e.sh
```

## Maintainer releases

Releases are deliberate, tag-ref based operations. Ordinary pushes and merges
do not run the release workflow. From a clean, up-to-date `main` checkout, run a bare command for the default
patch bump or choose another version operation explicitly:

```sh
mise run release
mise run release -- --patch
mise run release -- --minor
mise run release -- --major
mise run release -- --version 1.4.0
mise run release -- --minor --yes
```

For the first release when Cargo already contains the desired version and no
matching tag exists, use the exact-version form:

```sh
mise run release -- --version 0.1.0
```

The task performs a read-only preflight (clean worktree, `main`,
`HEAD == origin/main`, credentials, valid Cargo semver, and unused local and
remote tag), prints the exact plan, and then has two confirmations. A normal
release changes `Cargo.toml`/`Cargo.lock`, creates `Release skills vX.Y.Z`,
creates the annotated `vX.Y.Z` tag, and pushes `main` plus the tag. An exact
version equal to the current Cargo version is allowed only for this initial
untagged release: it tags and pushes the existing current commit without
writing Cargo files or creating a no-op release commit. The second confirmation
independently dispatches the external GitHub build. `--yes` accepts both
confirmations; `--version` accepts an optional leading `v` and is exclusive
with the bump flags.

If the dispatch fails after the tag is pushed, the tag is retained. Retry only
the external build with the printed command, or explicitly resume it:

```sh
mise run release -- --resume --version 1.4.0
mise run release -- --resume --version 1.4.0 --yes
```

Resume verifies that the pushed tag points at `HEAD == origin/main` and does
not edit, commit, tag, or push anything. Never delete or retag a published
version implicitly. The workflow is `workflow_dispatch` only and must be
started at `vX.Y.Z`; it derives and verifies the version from Cargo rather than
accepting a mutable workflow version input. It builds only Linux x86_64
(`x86_64-unknown-linux-gnu`) and macOS arm64 (`aarch64-apple-darwin`). The
release contains those archives, `SHA256SUMS`, and the generated immutable
compatibility manifest whose backend revision is the tagged commit SHA.

Each matrix build consumes an Actions runner, and the macOS arm64 runner is a
scarcer/costlier allowance than Linux. Use `--dry-run` only for mise's task
inspection; this release task has explicit confirmations instead of a hidden
remote action. Do not run a live dispatch while testing task changes: use fake
`git`, `cargo`, and `gh` commands and assert that only the intended
`workflow run --ref vX.Y.Z` invocation would occur.

Release assets are built by [`scripts/package-release.sh`](scripts/package-release.sh)
and [`.github/workflows/release.yml`](.github/workflows/release.yml). After a
public release exists, the read-only integration check can verify the real API,
manifest, archives, and checksums with
`RUN_RELEASE_INTEGRATION=1 tests/release_assets_integration.sh`. A local
developer uses `cargo run --manifest-path packages/skills/Cargo.toml`
or a separately published development artifact; no committed compatibility
manifest is a release source.
