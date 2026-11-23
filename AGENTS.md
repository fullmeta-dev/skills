Mise backend plugin and Rust CLI for managing Agent Skills.

Before using a Matt Pocock skill, load `adapt-matt-pocock-skills` and apply it
for the current harness.

## Agent skills

### Issue tracker

Issues are tracked in GitHub for `fullmeta-dev/skills`. See
`docs/agents/issue-tracker.md`.

### Triage labels

Use the default engineering triage labels. See
`docs/agents/triage-labels.md`.

### Domain docs

This repository uses a single-context layout. See `docs/agents/domain.md`.

## Agent harnesses
- Codex cli and Codex app
- [Pi coding agent](packages/coding-agent)
- Cursor ide

## Dev environment
- Use `mise` for toolchains/env/tasks
- Use `pitchfork` to start processes in reproducible manner e.g. watch and recompile.

## Test fixtures

Committed `SKILL.md` files under `tests/` are internal: set
`metadata.internal: true` so recursive scanners (`npx skills add
--full-depth`, used by `npx skills update`) skip them. Generate intentionally
invalid frontmatter in a `TempDir` inside the test instead of committing it.
See `tests/fixtures/README.md`.

## Git commit rules
- ASCII-only message.
- Subject line: short, imperative, no trailing period.
- Blank line, then body (wrap at 72 cols): explain why/what (not how).
- Avoid emoji, smart quotes, em dashes.
