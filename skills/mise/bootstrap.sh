#!/usr/bin/env bash
# Install the pinned mise-skills global config and bootstrap it with mise.
set -Eeuo pipefail

# The bootstrap script itself is pinned by the README URL. The downloaded
# config pins the plugin and is the release artifact users review.
REPOSITORY="https://raw.githubusercontent.com/fullmeta-dev/skills/main"
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
