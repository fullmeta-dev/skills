# Agent skills

This repository is both an Agent Skills source repository and a mise-managed
skills backend/CLI. The mise workflow installs complete immutable source
snapshots and exposes the selected skills through Pi, Claude, or the shared
Codex/Cursor target.

## mise-skills quick start

Prerequisites: [mise](https://mise.jdx.dev/) **2026.6.14 or newer**, Git, and
one supported agent. The bootstrap hook needs the minimum release's
`bootstrap.hooks.pre-tools` phase.

Install the pinned global config, backend plugin, and released CLI in one
copy-pasteable bootstrap. The script writes the checked-in config artifact
(`docs/mise-skills-bootstrap.toml`), installs its idempotent pre-tools helper,
and runs real `mise bootstrap`:

```sh
curl --fail --location https://raw.githubusercontent.com/fullmeta-dev/skills/336443fb9a50a035a2e4dc1a41cb0b450da29274/skills/mise/bootstrap.sh | bash
```

For an offline or reviewed setup, copy [`docs/mise-skills-bootstrap.toml`](docs/mise-skills-bootstrap.toml)
to `$MISE_CONFIG_DIR/config.toml` and
[`skills/mise/bootstrap-plugin.sh`](skills/mise/bootstrap-plugin.sh) to
`$MISE_CONFIG_DIR/mise-skills-bootstrap-plugin.sh`, then run `mise bootstrap`.
The config pins both the plugin declaration and CLI release; rerunning the
helper is a no-op when that exact plugin URL and revision are installed.

For a project, choose a source, selectors, and adapters. The CLI resolves the
latest branch or tag to a full immutable pin; do not calculate a `tree-` or
`rev:` value yourself:

```sh
mise run skills -- use fullmeta-dev/skills code-review --agent pi --agent agents
mise install
mise run skills -- sync --locked
mise run skills -- status
```

`use` writes the project `mise.toml`, preserves existing TOML, and adds a
postinstall hook. `--global` writes the user configuration; `--path FILE`
selects another config root. `agents` is the shared target used by Codex and
Cursor. Use `--agent claude` or `--agent pi` for their native targets.

A source is pinned when it is declared. `track` is only update intent and is
never followed by install or sync. Refresh explicitly, inspect the diff, and
then install/sync:

```sh
mise run skills -- upgrade
mise install
mise run skills -- sync --locked
```

Remove selected skills or the complete declaration. Owned links are removed;
unmanaged conflicts are reported and left untouched:

```sh
mise run skills -- remove skills:fullmeta-dev/skills code-review
mise run skills -- remove skills:fullmeta-dev/skills
mise run skills -- prune
```

Help and shell completion are side-effect free:

```sh
mise run skills -- --help
mise run skills -- completions bash > ~/.local/share/bash-completion/completions/mise-skills
mise run skills -- completions zsh > ~/.zfunc/_mise-skills
mise run skills -- completions fish > ~/.config/fish/completions/mise-skills.fish
mise run skills -- completions powershell > "$HOME/.config/powershell/mise-skills.ps1"
```

Use `mise tasks info skills` to inspect the task description and usage. The
canonical automation form is `mise run skills -- <command> [arguments]`; the
short `mise skills ...` form is convenient but may eventually be shadowed by a
mise built-in.

## Source syntax and references

Friendly source forms are `owner/repository`, `github:owner/repository`,
`git:https://host/repository.git`, and `path:relative-or-absolute-directory`.
Selectors are bare skill names, repository-relative globs, or tag expressions
such as `tag:web+frontend`; repeated includes are ORed and exclusions win.
Optional `.mise/skills.toml` files provide discovery roots, exclusions, tags,
and aliases.

The complete reference, including immutable versions, scope-local ownership,
adapter paths, conflicts, and troubleshooting, is in
[`docs/mise-skills.md`](docs/mise-skills.md).

## This repository as a source

The original Vercel `skills` CLI workflow remains supported:

```sh
skills add fullmeta-dev/skills --all
skills add . --all
skills list
skills update
skills remove
```

The mise workflow is complementary: it owns immutable source installation and
selection policy, while the backend materializes complete snapshots and the
CLI owns agent links.

## Development and compatibility checks

The shipped backend is under `skills/mise/plugin`; the Rust CLI is under
`skills/mise/mise-skills`. Run the isolated backend and compatibility checks
from the repository root:

```sh
skills/mise/mise-skills-tests/backend_test.sh
skills/mise/mise-skills-tests/e2e.sh
skills/mise/mise-skills-tests/run.sh
```

The harnesses use disposable `HOME`, config, data, state, and cache
directories. Set `MISE_SKILLS_KEEP_TEMP=1` to preserve failed fixtures. The
CLI unit, fixture, lifecycle, help, and snapshot-style integration tests run
with:

```sh
cargo test --manifest-path skills/mise/mise-skills/Cargo.toml
```
