# Skills

Personal skills for AI agents.

## Install

Install with Vercel's `skills` CLI:

```sh
skills add fullmeta-dev/skills --all
```

From a local checkout:

```sh
skills add . --all
```

Useful management commands:

```sh
skills list
skills update
skills remove
```

## mise compatibility baseline

The supported minimum mise release is **2026.6.14**. It is the first release
used here because it documents the `bootstrap.hooks.pre-tools` phase required
to install the backend before a `skills:*` tool is resolved. Older releases
are rejected with an upgrade instruction before skill installation starts.

Run the isolated Milestone 0 compatibility harness with:

```sh
skills/mise/mise-skills-tests/run.sh
```

The harness tests the minimum and latest stable mise releases in isolated
`HOME`, config, data, state, and cache directories. It exercises plugin
installation, bootstrap ordering, tool-option forwarding, no-op postinstall
convergence, the managed `mise-skills` executable, and
`mise run skills -- sync --locked` argument forwarding.
