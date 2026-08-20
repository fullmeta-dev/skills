#!/usr/bin/env bash
# Fail before bootstrap can ask mise to resolve a skills:* tool.
set -euo pipefail

required="${2:-${MISE_SKILLS_MINIMUM_MISE_VERSION:-2026.6.14}}"
mise_bin="${1:-mise}"
unset MISE_BIN MISE_SKILLS_MINIMUM_MISE_VERSION

version_output="$(MISE_NO_CONFIG=1 "$mise_bin" version 2>/dev/null || true)"
actual="$(printf '%s\n' "$version_output" | sed -nE 's/^([0-9]+\.[0-9]+\.[0-9]+).*/\1/p' | head -n1)"

version_at_least() {
    [ "$1" = "$2" ] && return 0
    local left right index
    IFS=. read -r -a left <<< "$1"
    IFS=. read -r -a right <<< "$2"
    for index in 0 1 2; do
        if (( 10#${left[index]:-0} > 10#${right[index]:-0} )); then
            return 0
        fi
        if (( 10#${left[index]:-0} < 10#${right[index]:-0} )); then
            return 1
        fi
    done
    return 0
}

if [ -z "$actual" ]; then
    printf 'mise-skills: unable to determine mise version from %q\n' "$mise_bin" >&2
    exit 78
fi

if ! version_at_least "$actual" "$required"; then
    cat >&2 <<EOF
mise-skills requires mise >= $required for the documented [bootstrap.hooks.pre-tools] phase; found $actual.
Upgrade mise before installing skills: https://mise.jdx.dev/installing-mise.html
EOF
    exit 78
fi
