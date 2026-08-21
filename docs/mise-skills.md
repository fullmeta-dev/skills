# mise-skills reference

`mise-skills` manages Agent Skills as immutable mise tool snapshots. The
normal workflow is:

```sh
mise run skills -- use SOURCE SELECTOR... --agent AGENT...
mise install
mise run skills -- sync --locked
```

The CLI chooses and resolves the source pin. Users choose the source,
selectors, and adapters; users do not calculate pins.

## Installation and scope

Use mise 2026.6.14 or newer. The global `skills` task is intentionally a
normal mise task:

```toml
[tasks.skills]
description = "Manage Agent Skills"
usage = '''
arg "[arguments]" var=#true help="mise-skills command and arguments"
'''
dir = "{{cwd}}"
run = "mise-skills ${usage_arguments-}"
raw = true
```

Use `mise run skills -- ...` in scripts and hooks. `mise skills ...` is an
interactive shorthand only. Bare invocation delegates to the CLI without
editing config, resolving sources, installing tools, or creating links. With
task `usage` metadata, mise reserves `mise run skills --help` for its own
side-effect-free task help; use `mise run skills -- --help` for CLI help.

Commands default to the nearest project `mise.toml`:

- `--global` uses the user config (`MISE_CONFIG_FILE` or
  `$MISE_CONFIG_DIR/config.toml`). Links go below `$HOME`.
- `--path PATH` uses exactly that file, or `mise.toml` inside a directory.
  Links are project-relative to the file's directory.

A config root owns only links recorded in its state manifest. Project and global
configurations are not merged by this CLI.

## Sources and immutable versions

Accepted source syntax:

```text
owner/repository
github:owner/repository
git:https://host.example/org/repository.git
git:ssh://host.example/org/repository.git
path:relative/or/absolute/directory
```

GitHub shorthand is written canonically as `github:owner/repository`; Git URLs
are written as `git:URL`. Local paths are resolved relative to the declaring
config file. `--subpath SUBPATH` is a safe source-relative directory used only
for discovery and selection. Absolute paths, `..`, backslashes, and escaping
links are rejected.

GitHub and Git sources resolve a branch, tag, or `--reference REF` to
`version = "rev:<full-40-character-commit>"`. Local sources resolve to
`version = "tree-<64-hexadecimal-digest>"`. These are implementation details:
`use` and `upgrade` write them automatically.

A declaration's `track` records update intent. Install, sync, and postinstall
never follow it. Only an explicit `upgrade` resolves the track again. `--locked`
prevents resolution and rewriting; it still permits mise to fetch an already
pinned Git commit. A changed local tree makes `sync --locked` fail until
`use` or `upgrade` refreshes the declaration.

## Discovery, catalog, and selectors

Every directory containing an exact `SKILL.md` is discovered recursively below
the source/subpath. `.git`, `node_modules`, `dist`, `build`, and `__pycache__`
are skipped. A complete skill directory includes scripts, references, assets,
and symlinks within its root. Duplicate frontmatter names use the lexical path
winner; `list --explain` reports shadowing.

`SKILL.md` requires YAML frontmatter with string `name` and `description`.
Top-level `tags` is an intentional extension accepted during normal install.
Run the non-blocking strict diagnostic separately:

```sh
mise run skills -- validate path:../skills-source --json
```

An optional `.mise/skills.toml` contains only discovery overlays:

```toml
version = 1

[discovery]
roots = ["skills", "packages/*/agent-skills"]
exclude = ["**/fixtures/**", "**/examples/**"]

[tags]
"skills/engineering/**" = ["engineering"]
"skills/engineering/web/**" = ["web"]

[aliases]
review = "skills/engineering/review/code-review"
```

Tags match `[a-z0-9][a-z0-9./-]*`. Selectors are:

- `tag:web` selects one tag;
- `tag:web+frontend` requires both tags;
- separate includes are ORed;
- a bare literal matches basename, frontmatter name, or alias;
- a selector containing `/`, `*`, `?`, or `**` is a repository-relative glob;
- excludes apply last and win; and
- missing selectors are stable warnings, not v1 failures.

Examples:

```sh
mise run skills -- list path:../skills-source --tree --explain
mise run skills -- list path:../skills-source --include tag:web --exclude tag:experimental
mise run skills -- use git:https://host.example/team/skills.git \
  'skills/engineering/**' --track main --agent agents
```

`list` is read-only and does not resolve or install a remote source. It accepts
local checkout/snapshot directories and can emit stable `--json` output.

## Agent targets

The `--agent` values are authoritative. `codex` and `cursor` are aliases for
the shared `agents` target, and duplicate operations are coalesced:

| adapter | global target | project target |
| --- | --- | --- |
| `agents` (`codex`, `cursor`) | `~/.agents/skills` | `.agents/skills` |
| `pi` | `~/.pi/agent/skills` | `.pi/skills` |
| `claude` | `~/.claude/skills` | `.claude/skills` |

Activation is a relative symlink to the installed complete artifact. Sync
creates, updates, and removes only links owned by the selected config root.

## Lifecycle

```sh
mise run skills -- use SOURCE SELECTOR --agent pi
mise install
mise run skills -- status --json
mise run skills -- upgrade skills:SOURCE
mise install
mise run skills -- sync --locked
mise run skills -- remove skills:SOURCE SELECTOR
mise run skills -- prune
```

- `use` edits source, version, selection, agents, and the postinstall hook;
- `mise install` installs the immutable artifact through the backend;
- `sync` verifies the artifact and converges selected links;
- `status` reports declaration and link state without changing anything;
- `upgrade` refreshes pins only and never installs or syncs;
- `remove` removes selected policy or a complete declaration, then delegates
  normal mise artifact removal when no config references that source+version;
- `prune` repairs stale links recorded for this config root without creating
  desired links.

## Ownership, conflicts, and recovery

Ownership state is stored under:

```text
$MISE_STATE_DIR/skills/<config-root-hash>.json
```

An existing unmanaged file, directory, or symlink is a conflict. Sync and
prune report it and leave it untouched. If a managed link is manually changed,
the change is also treated as a conflict. Restore or remove the target and run
`sync --locked` to converge it again.

If `mise unuse` removes a declaration without invoking this CLI, an owned link
may become dangling. `status` reports the dangling link and `prune` removes the
recorded stale link. A local source drift error is repaired by explicit
`use`/`upgrade`, followed by `mise install` and `sync --locked`.

Useful diagnostics:

```sh
mise run skills -- status --json
mise run skills -- list path:../skills-source --json --explain
mise run skills -- validate path:../skills-source
mise run skills -- prune
```

The backend verifies source identity, immutable versions, complete snapshot
inventory, safe links, and local tree digests. It does not store selectors,
agent choices, or project ownership in the artifact manifest.
