# Agent skills

This repository is an Agent Skills source tree and a mise backend/CLI. The
public executable is `skills`; it installs immutable source snapshots through
mise and materializes selected skills for supported agents.

## Quick start

Prerequisites: [mise](https://mise.jdx.dev/) 2026.6.14 or newer, Git, and a
supported agent. Install the backend plugin and the released CLI as ordinary
mise tools (no wrapper task is required):

```sh
mise plugins install skills https://github.com/fullmeta-dev/skills.git#348a5b2021d1e8b6d398c1da068fee90bb1c3653
mise use -g github:fullmeta-dev/skills@0.1.0
```

Set the CLI tool options in the global mise config:

```toml
[tools."github:fullmeta-dev/skills"]
version = "0.1.0"
bin = "skills"
```

Then install it through normal mise and select skills in a project:

```sh
mise install
skills use fullmeta-dev/skills code-review --agent pi
skills install
skills status
```

`skills use` resolves a full Git commit or local tree digest and writes the
selected config. `skills install` coordinates normal `mise install` for that
config, then runs a locked link sync. The generated postinstall hook is the
direct command `skills sync --locked`.

Use `--global`, `--path PATH`, and `--env ENV` to select the intended mise
configuration. The effective config is never inferred from an arbitrary tool
argument. `agents` is the shared Codex/Cursor target; `pi` and `claude` use
their native locations.

## Updates and removal

Source pins never move during install or sync. Refresh them explicitly, then
install the new immutable artifact:

```sh
skills upgrade
skills install
skills sync --locked
```

Update the CLI and its pinned backend as a compatible pair:

```sh
skills self-update
```

`self-update` reads [`release-compatibility.json`](release-compatibility.json),
installs the release before changing the selected config, and retains the old
config when any step fails. Inspect collisions before accepting a replacement:

```sh
skills doctor --global
skills install --accept-collision
```

Remove policy and owned links without touching unrelated targets:

```sh
skills remove skills:fullmeta-dev/skills code-review
skills remove skills:fullmeta-dev/skills
skills prune
```

## Help and completions

```sh
skills --help
skills completions bash > ~/.local/share/bash-completion/completions/skills
skills completions zsh > ~/.zfunc/_skills
skills completions fish > ~/.config/fish/completions/skills.fish
skills completions powershell > "$HOME/.config/powershell/skills.ps1"
```

The concise reference is [`docs/mise-skills.md`](docs/mise-skills.md).

## Development

The backend is under `skills/mise/plugin`; the Rust CLI is under
`skills/mise/mise-skills`:

```sh
cargo test --manifest-path skills/mise/mise-skills/Cargo.toml
skills/mise/mise-skills-tests/backend_test.sh
skills/mise/mise-skills-tests/e2e.sh
```

Release assets are built by [`scripts/package-release.sh`](scripts/package-release.sh)
and `.github/workflows/release.yml`. The first public release and its GitHub
release publication remain an explicit deployment step after review; this
checkout intentionally does not publish releases or tags.
