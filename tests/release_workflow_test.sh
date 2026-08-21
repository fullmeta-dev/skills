#!/usr/bin/env bash
# Static contract checks for release dispatch safety. No GitHub calls.
set -Eeuo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
workflow="$root/.github/workflows/release.yml"

# The only trigger is an explicit manual dispatch; it has no version/publish
# inputs and cannot run on ordinary pushes or merges.
grep -q '^  workflow_dispatch:$' "$workflow"
! grep -q '^  push:' "$workflow"
! grep -q '^    inputs:' "$workflow"
grep -q 'ref: \${{ github.ref }}' "$workflow"
grep -q 'GITHUB_REF_TYPE.*tag' "$workflow"
grep -q 'GITHUB_REF_NAME#v' "$workflow"
grep -q "tomllib.load(stream)\['package'\]\['version'\]" "$workflow"

# Keep the release target/cost surface intentionally narrow.
grep -q 'target: x86_64-unknown-linux-gnu' "$workflow"
grep -q 'target: aarch64-apple-darwin' "$workflow"
! grep -q 'windows\|x86_64-apple\|aarch64-unknown-linux' "$workflow"

# The generated asset is tied to the checked-out tag commit and published with
# checksums and both supported archives.
grep -q "'plugin_revision': tag_sha" "$workflow"
grep -q 'release-compatibility.json' "$workflow"
grep -q 'SHA256SUMS' "$workflow"
grep -q 'tag_name: \${{ github.ref_name }}' "$workflow"
printf 'release workflow contract checks passed\n'
