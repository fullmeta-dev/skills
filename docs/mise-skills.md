# `skills` reference

`skills` manages Agent Skills as immutable mise tool snapshots. It is a
normal mise-managed executable, not a task wrapper.

## Configuration and install

Install the pinned backend plugin into mise's plugin data, then declare the
CLI as an ordinary mise tool. The plugin does not need a `[plugins]` entry in
the selected config:

```sh
mise plugins install skills https://github.com/fullmeta-dev/skills.git#<plugin-commit>
mise use -g github:fullmeta-dev/skills@0.1.0
```

```toml
[tools."github:fullmeta-dev/skills"]
version = "0.1.0"
bin = "skills"
```

Then run:

```sh
mise install
skills use SOURCE SELECTOR... --agent AGENT...
skills install
```

`skills install` invokes normal `mise install` with an explicit effective
config path, then directly runs locked sync. It never passes a user-supplied
tool selector to mise. The selected environment is passed as mise's `-E`/
`MISE_ENV` context. Use `--global`, `--path PATH`, or `--env ENV`; global is
not merged into project ownership manifests.

Commands default to the nearest project `mise.toml`. `--env development`
writes/reads `mise.development.toml`, matching mise's environment-specific
configuration. A global environment uses `config.<env>.toml` below the mise
config directory. Diagnostics print the effective config path and environment.

## Commands

```text
skills list SOURCE [SELECTORS...]
skills use [--global|--path PATH|--env ENV] SOURCE [SELECTORS...] --agent AGENT...
skills install [--global|--path PATH|--env ENV] [--locked] [--dry-run]
skills sync [--global|--path PATH|--env ENV] [--locked]
skills status [--global|--path PATH|--env ENV] [--json]
skills upgrade [TOOL...]
skills self-update [--version VERSION]
skills remove [--global|--path PATH|--env ENV] TOOL [SELECTORS...]
skills prune [--global|--path PATH|--env ENV]
skills doctor [--global|--path PATH|--env ENV]
```

`use` edits only the selected config and resolves an immutable `rev:<commit>`
for Git sources or `tree-<sha256>` for local sources. `track` records update
intent only. `upgrade` is the explicit source-pin refresh; install, postinstall,
and sync never follow tracks. `self-update` reads the release compatibility
manifest, inspects the installed backend in mise's plugin data, and updates the
CLI/backend pair only after the new CLI is installed. A failed self-update
leaves the old declaration usable and does not add or rewrite a plugin
declaration in the selected config.

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

An existing unmanaged file, directory, or symlink is a collision. `sync` and
`install` report it and leave it untouched. A changed managed link is also a
collision. Run `skills doctor` to inspect PATH and mise owners without relying
on undocumented PATH precedence; use `--accept-collision` only after review.
`prune` removes stale links recorded for this config and never materializes
desired links.

## Release compatibility

`release-compatibility.json` pairs each CLI release with its minimum mise
version and exact backend plugin revision. The release workflow builds
platform assets named `skills_<version>_<target>.*`, checksums them, and can
publish them only when explicitly enabled. Release publication, tags, and
live asset verification are deployment steps after review, not performed by
this checkout.

For strict Agent Skills diagnostics without blocking normal discovery:

```sh
skills validate SOURCE --json
```
