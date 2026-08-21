# Settled design: skill discovery and catalog

This is the accepted discovery contract for the mise skills backend. It replaces
earlier exploratory alternatives that used categories, `name:` selectors,
collections, or `metadata`-encoded tags.

## Inputs

A source is an immutable Git checkout or a local directory snapshot. Discovery
runs against the selected source subpath and is identical for a Git checkout
and a local directory.

A skill is every directory containing an exact `SKILL.md`. Discovery walks to
arbitrary depth, excluding only hard implementation directories:

```text
.git, node_modules, dist, build, __pycache__
```

The complete directory is the skill artifact: `SKILL.md`, references, scripts,
and assets are all materialized together.

V1 permits overlapping roots: every `SKILL.md` is discovered. A duplicate
frontmatter `name` is resolved deterministically by lexical repository path;
the winner is reported by `list` and `status`. An optional catalog exclusion
can remove an unwanted candidate. This intentionally permits ordinary
shadowing rather than treating every nested layout as an error.

## Skill frontmatter

The interoperable baseline is the Agent Skills `SKILL.md` format: YAML
frontmatter with `name` and `description` followed by Markdown. The backend
adds direct portable tags:

```markdown
---
name: frontend-design
description: Design and review frontend interfaces.
tags:
  - web
  - frontend
  - design
---
```

`tags:` is a deliberate extension: most agents ignore it, while strict Agent
Skills validators may reject it as an unknown top-level field. Normal backend
operation accepts it. The optional strict validation command reports standard
conformance separately; it does not block normal installation.

Tags are non-empty lowercase tokens matching:

```text
[a-z0-9][a-z0-9./-]*
```

Invalid tags are errors; tags are not silently rewritten.

## Optional repository catalog

A source may contain `.mise/skills.toml`. It is a small overlay, not a list of
every skill:

```toml
version = 1

[discovery]
roots = ["skills", "packages/*/agent-skills"]
exclude = ["**/fixtures/**", "**/examples/**"]

# Matching values accumulate with SKILL.md tags.
[tags]
"skills/engineering/**" = ["engineering"]
"skills/engineering/web/**" = ["web"]
"skills/.experimental/**" = ["experimental"]

# A bare selector can refer to a stable short name.
[aliases]
review = "skills/engineering/review/code-review"
```

Without a catalog, the source subpath itself is the sole discovery root.

The catalog only provides:

- `discovery.roots` -- roots to scan instead of the source subpath;
- `discovery.exclude` -- paths not to scan;
- `[tags]` -- path-glob-to-tag mappings; and
- `[aliases]` -- a stable bare selector for one exact skill root.

It has no categories, collections, rule ordering, or explicit per-skill
inventory. Tags from all matching catalog entries and the skill frontmatter
are unioned.

## Selection grammar

There are only path selectors and tag selectors.

```toml
include = [
  "tag:web+frontend",
  "tag:project-management",
  "code-review",
  "skills/engineering/neo4j/**",
]
exclude = ["tag:experimental"]
```

- `tag:web` selects skills tagged `web`.
- `tag:web+frontend` selects skills carrying both tags.
- Separate `include` entries are an OR union.
- Every non-`tag:` selector is a skill-path selector.
- A bare literal matches a skill root basename, frontmatter name, or catalog
  alias. If several candidates match, lexical path ordering provides the
  deterministic shadowing winner and `list --explain` reports it.
- A selector containing `/`, `*`, `?`, or `**` is a glob against the
  repository-relative skill-root path.
- `exclude` is applied after `include` and wins.
- Missing includes and excludes are reported but are not errors in v1; this
  keeps evolving repositories and intentionally broad selectors convenient.

All discovery and matching is evaluated against the pinned source version, so
selectors are reproducible. Only `skills upgrade` resolves a new Git ref and
can therefore discover newly matching skills.

## Implementation approach

The `skills` executable owns YAML parsing, TOML catalog parsing, walking,
selection, and extraction. The vfox backend plugin only checks out an exact
source revision into mise's standard installation path.

Rust is a pragmatic CLI implementation choice, not a mise requirement: it
ships as a static executable and provides mature YAML/TOML/glob libraries. The
plugin itself remains Lua, which is the documented mise backend-plugin path.

## References

- [Agent Skills specification](https://agentskills.io/specification)
- [Vercel skills discovery](https://github.com/vercel-labs/skills/blob/main/src/skills.ts)
- [Vercel source parsing](https://github.com/vercel-labs/skills/blob/main/src/source-parser.ts)
- [mise backend plugins](https://mise.jdx.dev/backend-plugin-development.html)
- [mise tool options](https://mise.jdx.dev/dev-tools/index.html#tool-options)
