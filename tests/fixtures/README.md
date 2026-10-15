# Test fixtures

These trees are inputs for the `packages/skills` tests. They are not
publishable skills, so every committed `SKILL.md` here sets:

```yaml
metadata:
  internal: true
```

`npx skills add --full-depth` (used by `npx skills update`) walks every
directory, so an unflagged fixture would be offered as an installable skill and
a fixture with intentionally malformed frontmatter would warn on every update.

Keep the flag when adding fixtures. Frontmatter that must be invalid cannot be
committed here: generate it in a `TempDir` inside the test instead, as
`malformed_yaml_invalid_tags_and_escaping_links_are_reported` does in
`packages/skills/tests/discovery_fixture.rs`.
