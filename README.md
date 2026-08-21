# Agent skills

This repository is an Agent Skills source tree and a mise backend/CLI. The
public executable is `skills`; it installs immutable source snapshots through
mise and materializes selected skills for supported agents.

## Quick start

Prerequisites: [mise](https://mise.jdx.dev/) 2026.6.14 or newer, Git, and a
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

Mise shell activation is required for the global binary to be on `PATH`. Then
install it through normal mise and select skills in a project:

```sh
mise install
skills use fullmeta-dev/skills code-review --agent pi
skills install
skills status
```

If `skills` is not the expected executable, compare `mise which skills` and
`command -v skills`. `skills doctor --global` reports a foreign executable;
remove or reorder that program explicitly after review. The installer never
deletes a pre-existing `skills` executable.

`skills use` resolves a full Git commit or local tree digest and writes the
selected config. `skills install` coordinates normal `mise install` for that
config, then runs a locked link sync. The generated postinstall hook is the
direct command `skills sync --locked`.

Use `--global`, `--path PATH`, and `--env ENV` to select the intended mise
configuration. The effective config is never inferred from an arbitrary tool
argument. `agents` is the shared Codex/Cursor target; `pi` and `claude` use
their native locations.

## Updates and removal

Source pins never move during install or sync. Refresh them explicitly, then
install the new immutable artifact:

```sh
skills upgrade
skills install
skills sync --locked
```

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
skills remove skills:fullmeta-dev/skills code-review
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
`packages/skills`:

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
