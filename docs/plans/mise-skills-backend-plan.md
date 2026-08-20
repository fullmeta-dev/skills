# Plan: mise-managed Agent Skills

## Status

Final design for review before implementation.

This project adds a real **vfox backend plugin** for immutable skill-source
snapshots and a small **`mise-skills` CLI** for skill-specific commands. It
does not fork mise or require an upstream mise patch.

The settled discovery contract is in
[`../research/skill-discovery-and-catalog-design.md`](../research/skill-discovery-and-catalog-design.md).

## Product model

A skill source is a normal mise tool:

```toml
[tools."skills:fullmeta-dev/skills"]
source = "github:fullmeta-dev/skills"
version = "rev:7f2cb9258e..."
track = "main"
include = [
  "tag:web+frontend",
  "tag:project-management",
  "code-review",
  "skills/engineering/neo4j/**",
]
exclude = ["tag:experimental"]
agents = ["pi", "codex", "cursor"]
```

The `skills:` backend installs the entire immutable source snapshot. The
selection and agent options determine which skill directories are exposed from
that shared snapshot. Different projects may reuse one installed source
revision while selecting different skills.

- `source` plus `version` identify the artifact.
- `track` is update intent only; it is never followed by `install` or `sync`.
- `include`, `exclude`, and `agents` are activation policy.
- Changing `source` requires a new `skills:<source-id>` tool key. The CLI
  derives and validates that key; it does not silently repoint an existing key
  at a different source.

## Supported mise surfaces

### Native tool lifecycle

The vfox backend uses documented mise backend hooks:

```text
BackendListVersions
BackendInstall
BackendExecEnv (omitted unless a concrete need appears)
```

Normal mise commands manage source artifacts:

```sh
mise install
mise use skills:fullmeta-dev/skills@rev:<commit>
mise ls-remote skills:fullmeta-dev/skills
```

`mise unuse` is valid for removing a tool declaration, but it is not the
recommended user workflow because it cannot clean agent links. Users remove
skills through the skills CLI instead.

### Skill-specific CLI

The global mise config installs the CLI as a normal versioned tool and exposes
it through a normal task:

```toml
[tools]
"github:fullmeta-dev/mise-skills" = "0.1.0"

[tasks.skills]
description = "Manage Agent Skills"
dir = "{{cwd}}"
run = "mise-skills"
raw = true
```

The stable documented and scripted invocation is:

```sh
mise run skills -- <command> [arguments]
```

For interactive convenience, `mise skills ...` is supported as mise's task
shorthand. Mise documents that direct task shorthand can be shadowed by a
future built-in command, so it is never used in automation, generated hooks,
or documentation that must remain forward-compatible.

The CLI owns commands the generic tool interface cannot express naturally:

```sh
mise run skills -- list <source> [--tree] [--json]
mise run skills -- use [--global|--path PATH] <source> <selector...> --agent <agent...>
mise run skills -- sync [--global|--path PATH] [--locked]
mise run skills -- status [--json]
mise run skills -- upgrade [<skills-tool>...]
mise run skills -- remove [--global|--path PATH] <skills-tool> [<selector...>]
mise run skills -- prune [--global|--path PATH]
```

`use`, `upgrade`, and `remove` edit `mise.toml` with a TOML AST editor,
preserving comments, unrelated tables, and existing hooks.

## Bootstrap and machine setup

### Global configuration

The user-owned global mise config, tracked in dotfiles, declares:

1. the skills backend plugin source and exact plugin revision;
2. the `mise-skills` CLI release; and
3. the `skills` task above.

```toml
[plugins]
"vfox-backend:skills" = "https://github.com/fullmeta-dev/mise-skills.git#<plugin-commit>"

[tools]
"github:fullmeta-dev/mise-skills" = "0.1.0"
```

`[plugins]` records the desired plugin repository but does not itself install
an already-missing plugin on current mise releases. The bootstrap configuration
therefore installs it in a documented **pre-tools bootstrap hook**, before
`mise install` sees a `skills:*` entry:

```toml
[bootstrap.hooks.pre-tools]
run = "mise plugins install skills https://github.com/fullmeta-dev/mise-skills.git#<plugin-commit>"
```

The installer command is idempotent: it checks the installed plugin URL/ref
first and only replaces it when the pinned plugin revision differs. The
implementation must never use `--force` unconditionally.

`mise bootstrap` then installs the global `mise-skills` CLI and every active
`skills:*` source tool. This hook depends on the documented `pre-tools`
bootstrap phase. Milestone 0 pins the minimum mise release that provides it;
older mise versions fail with a clear upgrade instruction before attempting
skill installation.

### Project configuration

`mise run skills -- use` writes the source declaration and appends the
following ordinary mise postinstall hook to its owning config root:

```toml
[hooks]
postinstall = ["mise run skills -- sync --locked"]
```

Mise documents that general `postinstall` hooks run even when `mise install`
finds no missing tools. Therefore both ordinary installation and bootstrap
converge skills:

```text
mise install   -> source snapshots, then selected agent links
mise bootstrap -> pre-tools plugin install, tools, then selected agent links
```

The AST editor appends to an existing string/table/array hook without replacing
user-owned hooks. It never creates, renames, or overwrites a `bootstrap` task.

## Source handling

### Source types

```text
github:owner/repository
git:https://host.example/org/repository.git
path:relative-or-absolute-directory
```

The CLI accepts friendly GitHub shorthand (`owner/repository`), Git URLs, and
local paths, then writes one of the explicit forms above. It also accepts a
safe source subpath for every source type.

### Immutable versions

- GitHub sources resolve `track` or an explicit ref to a full commit and write
  `version = "rev:<full-commit>"`.
- Local sources compute a deterministic tree digest and write
  `version = "tree:<sha256>"`.
- `--locked` means do not resolve or rewrite a version. Fetching an absent
  already-pinned Git commit is allowed.
- `--locked` does not imply offline operation; it only prevents resolving or
  rewriting the declared version.
- A local source is resolved relative to the config file that declares it, not
  the invocation directory.

For a local digest, hash a sorted stream of repository-relative path, entry
type, executable bit, and file bytes or symlink target. Exclude mtimes,
ownership, `.git`, and backend state. A changed tree fails `sync --locked`
until the user explicitly runs `use` or `upgrade` to refresh the declaration.

### Backend installation

The Lua backend hook:

1. validates that the tool key, `source`, and `version` agree;
2. creates mise's supplied install path;
3. obtains the exact Git commit or validated local snapshot;
4. verifies the resulting revision/digest;
5. writes a small artifact manifest containing canonical source, revision, and
   full discovered-skill inventory/tree hashes.

The artifact manifest describes the repository snapshot only. It never stores
project-specific selection or agent-link choices.

## Discovery, catalog, and selection

The implementation follows the settled research document.

### Skill discovery

- Recursively discover every directory containing exact `SKILL.md` below the
  source subpath or catalog roots.
- Skip `.git`, `node_modules`, `dist`, `build`, and `__pycache__`.
- Install complete skill directories, including scripts, references, and
  assets.
- Permit nested candidates and deterministic shadowing: duplicate skill names
  resolve by lexical repository path and are explained by `list --explain`.
- Reject path traversal and symlinks escaping the discovered skill root.

### Frontmatter and tags

`name` and `description` are required YAML frontmatter fields. Top-level
`tags:` is supported deliberately:

```markdown
---
name: frontend-design
description: Design and review frontend interfaces.
tags: [web, frontend, design]
---
```

Tags must match `[a-z0-9][a-z0-9./-]*`; invalid values are errors rather than
being rewritten. Strict Agent Skills validation is an optional diagnostic, not
a normal-install gate, because top-level `tags:` is an intentional extension to
the current standard.

### Optional `.mise/skills.toml`

A repository may provide this small overlay:

```toml
version = 1

[discovery]
roots = ["skills", "packages/*/agent-skills"]
exclude = ["**/fixtures/**", "**/examples/**"]

[tags]
"skills/engineering/**" = ["engineering"]
"skills/engineering/web/**" = ["web"]
"skills/.experimental/**" = ["experimental"]

[aliases]
review = "skills/engineering/review/code-review"
```

It only specifies discovery roots/exclusions, path-derived tags, and aliases.
It is optional, non-exhaustive, and contains no categories, collections, or
per-skill inventory.

### Selectors

```toml
include = [
  "tag:web+frontend",
  "tag:project-management",
  "code-review",
  "skills/engineering/neo4j/**",
]
exclude = ["tag:experimental"]
```

- `tag:web` selects a tag.
- `tag:web+frontend` requires both tags.
- Separate includes are ORed.
- A literal bare selector matches basename, frontmatter name, or an alias.
- A selector containing `/`, `*`, `?`, or `**` matches a repository-relative
  skill-root glob.
- Exclusions apply last and win.
- Missing selectors are reported but do not fail in v1.

All selection runs against the pinned revision. Only `upgrade` changes the
revision and can discover newly matching skills.

## Agent activation

### Scope

Use normal mise scope, without a custom global/project merge engine:

- `--global` declarations materialize into global agent skill roots.
- project declarations materialize into project agent skill roots.
- normal mise config shadowing is accepted.
- a sync only creates/removes links that its own config root recorded.

### Initial adapters

Implement Pi, Codex, and Cursor using their documented/current skill
directories. Adapter paths are isolated behind a small adapter interface:

```text
id
roots(scope)
materialize(skill source, target)
remove(owned target)
```

Explicit `agents = [...]` is authoritative; detection is used only to suggest
agents interactively. Activation is a relative symlink to the mise
installation.

### Ownership and removal

For each config root, persist a small manifest under mise state recording only
links created for that root. `sync` updates or removes only those links.

```text
$MISE_STATE_DIR/skills/<config-root-hash>.json
```

V1 behavior:

- an existing unmanaged target is left untouched and reported as a conflict;
- `remove` rewrites the declaration, syncs that config root, then calls normal
  mise removal for an unreferenced artifact when appropriate;
- `prune` removes stale links recorded for the selected config root only;
- direct `mise unuse` may leave a dangling link; `status` reports it and
  `sync`/`prune` repairs it;
- no cross-project or global garbage collection is attempted.

## Implementation

### Repository layout

```text
mise-skills/
  plugin/
    metadata.lua
    hooks/backend_list_versions.lua
    hooks/backend_install.lua
  crates/mise-skills/
    src/
  tests/
    fixtures/
  docs/
```

The plugin is Lua because that is mise's documented backend-plugin interface.
The CLI is Rust because it is a normal mise-managed executable with mature
YAML, TOML-editing, glob, filesystem, and Git libraries. Rust is not embedded
or downloaded by plugin hooks.

### Milestone 0: compatibility contract

Build a disposable plugin and run it in isolated data/config directories.
Pin the exact minimum mise release only when all pass:

1. vfox backend installation from `mise plugins install` works;
2. `[plugins]` plus `bootstrap.hooks.pre-tools` installs the plugin before a
   `skills:*` tool is installed;
3. `[tools."skills:..."]` passes string, array, and nested options to the
   backend unchanged;
4. general postinstall hooks run after a no-op `mise install`;
5. the global `mise-skills` tool is available to the postinstall hook;
6. canonical `mise run skills -- sync --locked` forwards arguments unchanged.

Record the exact mise release in the README and fail early below it. The plan
depends only on documented backend plugins, tool options, bootstrap hooks, and
explicit `mise run` task invocation; it does not depend on legacy asdf plugin
commands or undocumented external-command dispatch.

### Milestone 1: backend and local vertical slice

Implement the Lua backend for `path:` sources and `tree:<sha256>` versions.
Implement a minimal Rust CLI that discovers one `SKILL.md`, materializes it for
Pi, records one config-root ownership manifest, and removes it.

Prove end-to-end:

```sh
mise run skills -- use ../skills-demo hello --agent pi
mise install
mise run skills -- status
mise run skills -- remove skills:local/skills-demo hello
```

Rerunning install/sync is a no-op.

### Milestone 2: Git source snapshots

Add `github:` and generic `git:` source resolution, safe source subpaths,
commit pinning, `rev:` versions, and `track` resolution for explicit CLI
upgrades. Test fresh-cache install, cached install, missing pinned commit
fetch, and an upgrade diff that changes only `version`.

### Milestone 3: discovery and selectors

Add arbitrary-depth walking, tags, lexical shadowing, catalog overlay, path
selectors, tag unions/intersections, exclusions, `list`, `--tree`, `--json`,
and `--explain`.

Use fixtures for nested paths, tags, aliases, catalog roots/exclusions,
shadowing, malformed YAML, invalid tags, and escaping links.

### Milestone 4: config editing and activation lifecycle

Implement AST-preserving `use`, `upgrade`, `remove`, and postinstall-hook
insertion. Test global/project/path targets, comments, all existing hook
shapes, repeated use, and hook preservation.

### Milestone 5: complete adapters and hardening

Complete Pi, Codex, and Cursor adapter coverage. Test scope-local ownership,
collisions, dangling links caused by direct
`mise unuse`, and `prune`. Add strict Agent Skills validation as a diagnostic
command and release documentation.

## Validation

- Unit tests: source parsing, revisions/digests, tag grammar, catalog parsing,
  selectors, TOML edits, and ownership manifests.
- Fixture tests: ordinary/nested/overlapping skill trees, catalog overlays,
  unsafe links, local source drift, and selector shadowing.
- Integration tests: isolated `HOME`, `MISE_DATA_DIR`, `MISE_STATE_DIR`, and
  `MISE_CONFIG_DIR`; plugin install; `mise bootstrap`; global/project configs;
  rerunnable install; remove/prune recovery.
- CLI tests: help, JSON, dry-run, no-op convergence, and stable diagnostics.
- Compatibility tests: current supported mise minimum plus the latest stable
  mise release; plugin and CLI release pinning.

## References

- [mise backend plugin development](https://mise.jdx.dev/backend-plugin-development.html)
- [mise tool options](https://mise.jdx.dev/dev-tools/index.html#tool-options)
- [mise plugins](https://mise.jdx.dev/plugins.html)
- [mise bootstrap](https://mise.jdx.dev/bootstrap.html)
- [mise hooks](https://mise.jdx.dev/hooks.html)
- [mise task invocation](https://mise.jdx.dev/tasks/running-tasks.html)
- [Agent Skills specification](https://agentskills.io/specification)
- [Vercel skills discovery](https://github.com/vercel-labs/skills/blob/main/src/skills.ts)
- [Vercel source parser](https://github.com/vercel-labs/skills/blob/main/src/source-parser.ts)
