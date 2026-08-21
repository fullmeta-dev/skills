#!/usr/bin/env bash
# Install the pinned mise-skills global config and bootstrap it with mise.
set -Eeuo pipefail

# README passes the same immutable ref used to fetch this script.
if [[ $# -ne 1 || -z "${1:-}" ]]; then
    printf 'usage: bootstrap.sh <immutable-ref>\n' >&2
    exit 2
fi
BOOTSTRAP_REF="$1"
BASE_URL="${MISE_SKILLS_BOOTSTRAP_BASE_URL:-https://raw.githubusercontent.com/fullmeta-dev/skills}"
REPOSITORY="${BASE_URL%/}/$BOOTSTRAP_REF"
CONFIG_DIR="${MISE_CONFIG_DIR:-${XDG_CONFIG_HOME:-${HOME:?}/.config}/mise}"
mkdir -p "$CONFIG_DIR"
curl --fail --location --silent --show-error \
    "$REPOSITORY/skills/mise/bootstrap-plugin.sh" \
    --output "$CONFIG_DIR/mise-skills-bootstrap-plugin.sh"
chmod 755 "$CONFIG_DIR/mise-skills-bootstrap-plugin.sh"
curl --fail --location --silent --show-error \
    "$REPOSITORY/docs/mise-skills-bootstrap.toml" \
    --output "$CONFIG_DIR/config.toml"
mise bootstrap --yes
