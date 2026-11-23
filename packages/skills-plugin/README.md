# skills mise backend

This Lua vfox backend installs one complete immutable source snapshot for a
`skills:<source-id>` tool. Selection and agent activation belong to the
`skills` CLI, not this plugin.

## Tool options

A tool declaration supplies the canonical source and, optionally, a safe
source-relative subpath:

```toml
[tools."skills:owner/skills"]
source = "github:owner/skills"
version = "rev:<40-character-commit>"
source_subpath = "skills"
```

Local `path:` sources are resolved against `config_root` when it is supplied
by the caller (the backend also accepts the mise project/config root
environment values). Their immutable version is
`tree-<64-character-sha256>`. This is an internal mise-compatible encoding:
the `skills` lifecycle commands resolve and write it; users must
not calculate or hand-write a local digest. There is no supported manual
local-digest workflow in this backend milestone.
GitHub shorthand is converted to a Git URL; `git:` accepts HTTPS, SSH, and
file URLs. `track` is intentionally ignored by installation.

The backend validates the source identity represented by the tool key, rejects
unsafe subpaths and links escaping the source root, fetches missing pinned Git
commits, and verifies the copied tree. Each install writes
`.mise-skills-manifest.json` containing only source, revision, tree, and
snapshot skill inventory facts. Selection and agent-link options are never
written to that manifest.

`BackendListVersions` returns raw full commit IDs for GitHub and explicit Git
keys because native mise rejects the custom `rev:` prefix in list results.
`BackendInstall` accepts either that raw value or `rev:<full-commit>` and
records the canonical immutable revision in the artifact manifest.
