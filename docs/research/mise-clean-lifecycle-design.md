# Research: cleaner mise-skills lifecycle design

## Summary

A clean design is to publish the CLI as the real `skills` executable and let
mise manage it as an ordinary GitHub release tool. Keep the source backend as a
pinned mise plugin, but do not require a task or a `mise-skills` wrapper for
normal use. `mise upgrade` can update declared tools, but it cannot by itself
coordinate plugin replacement, CLI self-update, source-pin refresh, and skill
link materialization; one explicit `skills update` coordinator is therefore
needed if the product promises one update command.

The repository's current bootstrap declares CLI `0.1.0`, but the checkout has
no release workflow or release asset and the real E2E uses a local HTTP release
fixture. The first release must exist before the documented bootstrap can work.

## Findings

### 1. Plugin lifecycle and refs (verified)

- `mise plugins install NAME [URL]` installs a plugin; a Git URL may carry a
  `#REF`. `mise plugins update [NAME]` updates an installed plugin from its
  configured origin, and `mise plugins uninstall NAME` removes the installed
  plugin. These are plugin operations, not tool-version operations. See the
  official command reference: [`plugins`](https://mise.jdx.dev/cli/plugins.html)
  and [plugin development](https://mise.jdx.dev/dev-tools/plugins.html).
- A ref in a Git URL selects the plugin checkout revision/branch/tag; it is not
  the version of a tool provided by that plugin. A full commit is the safest
  reproducible ref. `plugins update` is inherently an intentional movement of
  that ref; it should not be confused with installing a tool at a pinned
  version.
- This repository registers the plugin as `vfox-backend:skills`, while the
  plugin's metadata name is `skills` (`skills/mise/plugin/metadata.lua`). The
  bootstrap helper installs the named plugin at an exact URL+commit and
  explicitly uninstalls/reinstalls when either changes
  (`skills/mise/bootstrap-plugin.sh`). It does not use `--force`.

### 2. Tool lifecycle and config target (verified)

- `mise use` writes a tool declaration; `mise install` installs declarations;
  `mise upgrade` changes selected declarations to newer versions and installs
  them when followed by `mise install`; `mise uninstall` removes installed
  tool versions. `use` and `install` are deliberately separate in mise's
  model. [`use`](https://mise.jdx.dev/cli/use.html),
  [`install`](https://mise.jdx.dev/cli/install.html),
  [`upgrade`](https://mise.jdx.dev/cli/upgrade.html), and
  [`uninstall`](https://mise.jdx.dev/cli/uninstall.html) are the authoritative
  command references.
- The normal project target is the nearest project `mise.toml`. `-g/--global`
  targets the user config; `--path` targets an explicit config path. The
  repository's CLI independently implements the same three scopes: project,
  `--global`, and `--path` (`skills/mise/mise-skills/src/config.rs`), and its
  docs explicitly say that project and global link ownership are not merged
  (`docs/mise-skills.md`). Mise's configuration search/precedence is documented
  at [configuration](https://mise.jdx.dev/configuration.html).
- Environment selection and profiles are context selectors, not additional
  artifact identities. `--env`/`MISE_ENV` selects an environment-specific mise
  configuration; `--profile` selects a mise profile/context. They change which
  declarations are read, while `--global` versus project/`--path` changes where
  the declaration is written. See [environments](https://mise.jdx.dev/environments.html)
  and [profiles](https://mise.jdx.dev/profiles.html). A design should always
  print the effective config path, environment, and profile in diagnostics.

### 3. Pinning, lockfiles, and `latest` (verified)

- A concrete tool version is reproducible; `latest` is a moving selector and
  is resolved when mise needs to resolve it. `mise use --pin` records the
  resolved concrete version rather than retaining a moving selector. Mise's
  lockfile support records resolved tool versions for repeatable installs;
  `mise install --locked` refuses to change the lock rather than silently
  resolving new versions. See [lockfiles](https://mise.jdx.dev/cli/lock.html),
  [tool versions](https://mise.jdx.dev/dev-tools/), and
  [latest versions](https://mise.jdx.dev/dev-tools/versions.html).
- `mise upgrade` is the explicit operation for moving versions. A config that
  says `latest` remains policy, while a lockfile can keep an install stable
  until an explicit lock/upgrade operation. Therefore “install” must not mean
  “follow every branch/tag again.”
- The current repository has a stronger source-specific pin policy: GitHub/Git
  sources resolve a ref to `rev:<40-hex commit>`, local sources to
  `tree-<64-hex digest>`, and only `mise-skills upgrade` follows `track`
  (`docs/mise-skills.md`; `skills/mise/mise-skills/src/source.rs`). The shipped
  plugin installs a complete source snapshot and verifies its digest
  (`skills/mise/plugin/lib/skills_backend.lua`). This is good source-locking
  behavior, but it is separate from mise's own tool lockfile.

### 4. GitHub backend naming and assets (verified)

- The GitHub backend treats `github:OWNER/REPO` as a release-backed tool. Its
  `version` selects a release, `asset_pattern` selects the release asset, and
  `bin` names the executable exposed by the installed tool. The backend does
  not infer that the repository's GitHub name and executable name are the same.
  See the official [GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html).
- To expose a binary named `skills`, the release must contain an executable
  asset (or archive member) that the backend can select, and the declaration
  must use `bin = "skills"`; the release asset filename must match the chosen
  `asset_pattern`. For platform releases, publish one unambiguous asset per
  OS/architecture and make the pattern discriminate them. Do not rely on the
  current `bin = "mise-skills"` mapping.
- The checked-in bootstrap currently declares:
  `[tools."github:fullmeta-dev/skills"]`, `version = "0.1.0"`,
  `bin = "mise-skills"`, and `asset_pattern = "mise-skills"`
  (`docs/mise-skills-bootstrap.toml`). The E2E fixture deliberately supplies a
  local fake release and checks the normal GitHub API release lookup
  (`skills/mise/mise-skills-tests/e2e.sh`). The expected public release URL is
  [`fullmeta-dev/skills/releases`](https://github.com/fullmeta-dev/skills/releases);
  the required `v0.1.0` release is currently missing, so a real bootstrap
  cannot download this tool until it is published.

### 5. Is a task necessary? (verified, then recommendation)

A mise task is not required to run a mise-managed executable. Once the release
is installed with `bin = "skills"`, users can invoke `skills ...` through the
mise-managed PATH. A task is required only for the alternate spelling
`mise run skills -- ...`, task metadata/help, or a deliberately stable command
alias. The existing task is explicit in `docs/mise-skills.md` and the global
bootstrap TOML; it exists because the shipped executable is currently named
`mise-skills` and the desired task command needs argv forwarding.

**Recommendation:** remove the task from the required architecture. It may
remain as an optional convenience alias, but direct `skills` should be the
canonical documented command. This eliminates a wrapper process and avoids
coupling the CLI to mise's future task namespace.

### 6. Collision behavior and diagnostics (verified gap + recommendation)

- `bin = "skills"` creates a real name collision risk with the existing Vercel
  `skills` CLI, another mise tool exposing `skills`, or an unrelated executable
  earlier on `PATH`. The current repository intentionally avoids this by
  exposing `mise-skills` (`docs/mise-skills-bootstrap.toml`). No collision test
  or collision policy is present in `skills/mise/mise-skills-tests`; the source
  also does not claim ownership of the global `skills` name.
- Mise's supported inspection primitives are `mise which skills` (which
  executable mise resolves) and `mise ls --current`; use `command -v skills`
  to inspect the shell's final PATH resolution. See [which](https://mise.jdx.dev/cli/which.html)
  and [ls](https://mise.jdx.dev/cli/ls.html). The official docs do not promise
  a portable precedence rule for two tools exporting the same binary name.
  Consequently, treating “last installed wins” as behavior would be unsafe.
- **Recommendation:** before installation, detect an existing `skills` owner;
  after activation, print the selected owner, resolved path, tool/version, and
  shadowed candidates. If mise cannot provide owner-level collision data,
  `skills doctor` (or the install diagnostic) should report both
  `mise which skills` and the pre-existing PATH result and fail unless the user
  explicitly accepts replacement. Add an integration fixture with two tools
  exporting `skills`; assert deterministic failure/diagnostics rather than
  asserting undocumented precedence.

## Recommended no-wrapper architecture and lifecycle

1. **Publish first.** Create `v0.1.0` (and later releases) with an executable
   named `skills`, platform-specific assets, checksums/signatures, and release
   names that exactly match one documented `asset_pattern`. The release must
   be tested through the real GitHub API, not only the fixture.
2. **Install infrastructure.** Pin and install the `skills` backend plugin at a
   reviewed full commit. Declare the CLI as an ordinary GitHub tool with
   `bin = "skills"` and a concrete version. Run `mise install --locked` (or
   `mise install` on a deliberately unlocked bootstrap) and verify with
   `mise which skills`.
3. **Install actual skills.** Invoke the installed binary directly:
   `skills use OWNER/REPO SELECTOR --agent AGENT`; then run `mise install` for
   the generated source declaration and `skills sync --locked` to materialize
   links. `use` chooses/resolves the source pin; install fetches the immutable
   artifact; sync changes only links owned by that config root. This lifecycle
   is already represented by `skills/mise/mise-skills/src/main.rs` and
   `docs/mise-skills.md`.
4. **Update actual skills.** `skills upgrade` refreshes source pins only;
   `mise install` fetches the new snapshots; `skills sync --locked` converges
   links. Do not make postinstall silently follow `track`.
5. **One-command update promise.** `mise upgrade` alone can update declared
   tool versions, but it does not semantically perform plugin replacement,
   invoke the skills CLI's source upgrade policy, or guarantee link sync.
   If one command is required, implement `skills update` as an explicit
   coordinator (plugin update according to a compatibility policy, CLI tool
   upgrade, source-pin refresh, install, then locked sync). It is a command in
   the actual CLI, not a `mise-skills` wrapper. Keep `mise upgrade` available
   for users who only want ordinary mise tools.
6. **Compatibility.** Publish a compatibility matrix tying each CLI release to
   the minimum mise version and plugin API/revision. The checkout currently
   documents mise `2026.6.14+` and tests minimum/latest/older behavior in
   `skills/mise/mise-skills-tests/run.sh`; preserve that gate. A coordinator
   must update the plugin before using backend features it requires, and must
   refuse incompatible CLI/plugin combinations with an actionable message.

## Sources

- Kept: [mise CLI reference](https://mise.jdx.dev/cli/) — primary command
  semantics.
- Kept: [mise plugins](https://mise.jdx.dev/cli/plugins.html) and
  [plugin development](https://mise.jdx.dev/dev-tools/plugins.html) — install,
  update, uninstall, and backend/plugin model.
- Kept: [mise GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html)
  — tool keys, release assets, `bin`, and asset matching.
- Kept: [mise configuration](https://mise.jdx.dev/configuration.html),
  [lockfiles](https://mise.jdx.dev/cli/lock.html), and
  [tool versions](https://mise.jdx.dev/dev-tools/versions.html) — target and
  version-resolution behavior.
- Kept: repository paths `docs/mise-skills.md`,
  `docs/mise-skills-bootstrap.toml`, `skills/mise/plugin/`,
  `skills/mise/mise-skills/src/`, and `skills/mise/mise-skills-tests/` — current
  implementation and fixture evidence.
- Dropped: third-party blog posts, package listings, and commentary — not
  primary sources and unnecessary for this decision.

## Gaps

The official documentation does not specify a cross-platform precedence rule
for two installed tools exporting the same executable name. Verify the exact
collision diagnostic against the supported mise version(s), then encode that
behavior in the integration fixture before making `skills` the default name.
Also verify the final release asset naming against the live GitHub backend and
publish the missing `v0.1.0` release.

## Acceptance report

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Wrote the requested focused research artifact only; no implementation or issue files were changed."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Brief separates verified facts, gaps, and recommendations and cites official mise URLs plus current repository paths."
    }
  ],
  "changedFiles": ["research.md"],
  "testsAddedOrUpdated": [],
  "commandsRun": [],
  "validationOutput": ["Research brief written to /home/vlad/code/fullmeta/skills/research.md"],
  "residualRisks": ["Exact duplicate-bin precedence is not promised by official mise docs and still requires a live-version integration check.", "The v0.1.0 GitHub release is not yet published."],
  "noStagedFiles": true,
  "diffSummary": "Added one cited mise lifecycle/design research brief.",
  "reviewFindings": ["no blockers"]
}
```