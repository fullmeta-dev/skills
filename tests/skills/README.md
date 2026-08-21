# `skills` compatibility harness

The test scripts use disposable `HOME`, `MISE_CONFIG_DIR`, `MISE_DATA_DIR`,
`MISE_STATE_DIR`, and `MISE_CACHE_DIR` directories. They exercise the shipped
backend through real mise and invoke the public `skills` binary directly.

Run the complete hermetic checks from the repository root:

```sh
tests/skills/run.sh
```

The backend-only fixture covers deterministic local trees, drift and traversal
rejection, escaping links, generic Git commits, GitHub shorthand, cache reuse,
and complete snapshot manifests:

```sh
tests/skills/backend_test.sh
```

The real-mise backend integration installs the shipped plugin and exercises
`mise ls-remote` and `mise install` for GitHub and local fixtures:

```sh
tests/skills/backend_mise_test.sh
```

The lifecycle E2E verifies direct public invocation, normal `mise install`
coordination, postinstall sync, environment-specific config targeting, local
pinning and drift, adapters, ownership, and deterministic executable/link
collision diagnostics:

```sh
cargo build --manifest-path packages/skills/Cargo.toml
tests/skills/e2e.sh
```
