#!/usr/bin/env bash
# Install the pinned mise-skills global config and bootstrap it with mise.
set -Eeuo pipefail

REPOSITORY="https://raw.githubusercontent.com/fullmeta-dev/skills/fb2eecb5da7e6670e4cf018ec7b3c5e9de7a24d7"
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
