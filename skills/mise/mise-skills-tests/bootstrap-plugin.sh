#!/usr/bin/env bash
# Compatibility-harness entry point for the shipped bootstrap helper.
set -Eeuo pipefail
exec "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/bootstrap-plugin.sh" "$@"
