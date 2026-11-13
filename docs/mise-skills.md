# `skills` reference

`skills` manages Agent Skills as immutable mise tool snapshots. It is a
normal mise-managed executable, not a task wrapper.

## Local product development

From this repository, build the local CLI and link mise's `skills` backend to
the working tree:

```sh
mise run link
mise run build
mise watch build # optional Rust rebuild loop
```

The repository's mise environment prepends
`packages/skills/target/debug` to `PATH`, so this checkout's debug CLI shadows
a released global CLI only while working here. `link` intentionally does not
build; it changes mise's global plugin link only. Lua/backend edits take effect
immediately through that link. Once a release has been published, run
`skills self-update` outside this checkout to restore mise's global plugin and
CLI to the latest compatible released pair.

## Configuration and install

Install the pinned backend plugin into mise's plugin data, then declare the
CLI as an ordinary mise tool. The plugin does not need a `[plugins]` entry in
the selected config:

```sh
mise plugins install skills https://github.com/fullmeta-dev/skills.git#<compatible-plugin-commit>
mise use --global --pin github:fullmeta-dev/skills@<release>
```

```toml
[tools."github:fullmeta-dev/skills"]
version = "<release>"
bin = "skills"
```

Mise shell activation is required for the global binary to be on `PATH`.
Then browse a source before selecting anything. `list` is read-only and
prints each skill's name, description, tags, aliases, artifact files, source,
revision, and selector decisions:

```sh
skills list fullmeta-dev/skills --json --explain
skills list path:../my-skills --include tag:web
skills use SOURCE SELECTOR... --agent AGENT...
skills install
skills status --json
```

`list` accepts the same source grammar as `use`: a local `path:DIR` or
ordinary directory, `owner/repository`, `github:owner/repository`, or a
`git:`/`https:`/`ssh:`/`file:` URL. Git sources are fetched at a resolved full
commit in a temporary checkout and removed after the read-only operation. Use
`--track BRANCH_OR_TAG` or `--reference REF` to preview a particular revision.
Selectors can be names, aliases, `tag:name`, or repository-relative globs;
use `--explain` to see matching, shadowing, and missing-selector decisions.
A source repository can publish a focused catalog with `.mise/skills.toml`:
set `[discovery].roots`, `[discovery].exclude`, `[tags]`, and `[aliases]`.
The catalog is read-only metadata for discovery; `skills use` still records the
immutable source snapshot in the selected mise config.

Use `mise which skills` and `command -v skills` to diagnose PATH collisions.
`skills doctor --global` reports a foreign executable; remediation is an
explicit user decision and no existing executable is removed automatically.

`skills install` invokes normal `mise install` with an explicit effective
config path, then directly runs locked sync. It never passes a user-supplied
tool selector to mise. The selected environment is passed as mise's `-E`/
`MISE_ENV` context. Use `--global`, `--path PATH`, or `--env ENV`; global is
not merged into project ownership manifests. The generated postinstall hook is
scope-aware: a project declaration writes `skills sync --locked`, while a
global or `--path` declaration writes the matching `--global`/`--path` flags,
because mise runs the hook with the current project directory as its cwd.

Commands default to the nearest project `mise.toml`. `--env development`
writes/reads `mise.development.toml`, matching mise's environment-specific
configuration. A global environment uses `config.<env>.toml` below the mise
config directory. Use `--path PATH` for an explicit file or directory. Commit
project/profile configs as ordinary mise files; use the normal uncommitted
`mise.local.toml` or `mise.<env>.local.toml` for machine-specific choices.

```sh
skills use --global fullmeta-dev/skills mise --agent agents
skills use --env development fullmeta-dev/skills mise --agent pi
skills use --path mise.local.toml fullmeta-dev/skills mise --agent pi
```

Diagnostics print the effective config path and environment.

## Commands

```text
skills list SOURCE [SELECTORS...]
# SOURCE may be path:DIR, owner/repository, github:owner/repository, or git URL
# Add --track REF / --reference REF for a read-only remote preview
skills use [--global|--path PATH|--env ENV] SOURCE [SELECTORS...] --agent AGENT...
skills install [--global|--path PATH|--env ENV] [--locked] [--dry-run]
skills sync [--global|--path PATH|--env ENV] [--locked]
skills status [--global|--path PATH|--env ENV] [--json]
skills upgrade [TOOL...]
skills self-update [--version VERSION] [--dry-run]
skills remove [--global|--path PATH|--env ENV] TOOL [SELECTORS...]
skills prune [--global|--path PATH|--env ENV]
skills doctor [--global|--path PATH|--env ENV]
```

`list` never edits a config. Its JSON/text output includes `source`, immutable
`version`, remote `revision` and `track` when applicable, and each skill's
`name`, `description`, `tags`, `aliases`, and artifact `files`.

`use` edits only the selected config and resolves an immutable `rev:<commit>`
for Git sources or `tree-<sha256>` for local sources. `track` records update
intent only. `upgrade` is the explicit source-pin refresh; install, postinstall,
and sync never follow tracks. `self-update` queries the latest GitHub release,
then consumes that release's immutable `release-compatibility.json` asset. It
inspects the installed backend in mise's plugin data and updates the CLI/backend
pair only after the new CLI is installed. A failed self-update leaves the old
declaration usable and does not add or rewrite a plugin declaration in the
selected config. Set `SKILLS_RELEASE_API` to a local API JSON fixture or
`SKILLS_COMPATIBILITY_MANIFEST` to a local manifest for deterministic,
no-network tests. `skills self-update --dry-run` prints the compatible plugin,
CLI, and config plan without installing or editing.

## Sources and selectors

Accepted sources are `owner/repository`, `github:owner/repository`,
`git:https://host/org/repository.git`, and
`path:relative/or/absolute/directory`. Local paths are relative to the config
that declares them. `--subpath` must be relative and cannot contain `..`,
backslashes, or an absolute prefix.

Selectors are bare names, repository-relative globs, or tags:

- `tag:web` selects one tag;
- `tag:web+frontend` requires both tags;
- separate includes are ORed;
- a bare selector matches basename, frontmatter name, or alias;
- `/`, `*`, `?`, and `**` select repository-relative paths; and
- excludes always win. Missing selectors are stable warnings.

Discovery recursively finds exact `SKILL.md` files while skipping `.git`,
`node_modules`, `dist`, `build`, and `__pycache__`. Duplicate names use the
lexical path winner and are explained by `list --explain`. Complete skill
directories (scripts, references, assets, and safe in-root links) are exposed.
An optional `.mise/skills.toml` supplies discovery roots, exclusions, tags, and
aliases only.

## Agent targets and ownership

| adapter | global target | project target |
| --- | --- | --- |
| `agents`, `codex`, `cursor` | `~/.agents/skills` | `.agents/skills` |
| `pi` | `~/.pi/agent/skills` | `.pi/skills` |
| `claude` | `~/.claude/skills` | `.claude/skills` |

Activation uses relative symlinks into the immutable mise installation. Each
selected config root owns only links recorded in:

```text
$MISE_STATE_DIR/skills/<config-root-hash>.json
```

Because those links resolve only on the machine that created them, project
sync also lists the generated names in the repository's `.git/info/exclude`
(a clone-local file, never committed) inside a marked `skills` block it keeps
up to date. They therefore do not appear in `git status`. A linked worktree is
left alone, because its `info/exclude` is shared with the main checkout.

An existing unmanaged file, directory, or symlink is a collision. `sync` and
`install` report it and leave it untouched. A changed managed link is also a
collision. Run `skills doctor` to inspect PATH and mise owners without relying
on undocumented PATH precedence; use `--accept-collision` only after review.
`prune` removes stale links recorded for this config and never materializes
desired links.

## Maintainer release operations

The only release entrypoint is the executable mise file task. Run it from a
clean, current `main`:

A bare `mise run release` defaults to a patch bump. Explicit operations are:

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

`--version` normalizes an optional leading `v` and is mutually exclusive with
the bump flags. Preflight checks the worktree, branch, `HEAD == origin/main`,
Git/GitHub credentials, Cargo semver, and local/remote tag availability before
writing. Without `--yes`, confirmation one creates the Cargo version/lockfile
change, `Release skills vX.Y.Z` commit, annotated tag, and pushes `main` plus
the tag. For an exact version equal to the current Cargo version, the initial
untagged release instead tags and pushes the existing current commit without
writing Cargo files or creating a no-op release commit. Confirmation two
separately dispatches the external workflow.

The workflow is `workflow_dispatch` only and must be dispatched at `vX.Y.Z`.
It derives and verifies Cargo's version, builds only Linux x86_64
(`x86_64-unknown-linux-gnu`) and macOS arm64 (`aarch64-apple-darwin`), and
publishes archives, `SHA256SUMS`, and a generated
`release-compatibility.json`. That manifest contains the tag commit SHA as the
immutable plugin revision. Each matrix job consumes an Actions runner; macOS
arm64 is the costlier/scarcer allowance, so avoid unnecessary dispatches.

If dispatch fails after a tag is pushed, history is retained. Resume only the
published matching tag without editing or pushing:

```sh
mise run release -- --resume --version 1.4.0 --yes
```

The printed `gh workflow run release.yml --repo fullmeta-dev/skills --ref
vX.Y.Z` command is also the exact retry operation. Tests must use fake
`git`/`cargo`/`gh` commands and never dispatch a live workflow. After a public
release exists, the read-only API/assets check is opt-in:
`RUN_RELEASE_INTEGRATION=1 tests/release_assets_integration.sh`. Ordinary
pushes and merges never trigger release builds; there is no committed
compatibility manifest to edit.

For strict Agent Skills diagnostics without blocking normal discovery:

```sh
skills validate SOURCE --json
```
