#!/usr/bin/env bash
# Install the pinned backend plugin without a forced reinstall.
set -euo pipefail

plugin_name="${1:?plugin name is required}"
desired_url="${2:?plugin URL is required}"
desired_ref="${3:?plugin revision is required}"
plugin_dir="${MISE_DATA_DIR:-${HOME:?}/.local/share/mise}/plugins/$plugin_name"
log_path="${SKILLS_COMPAT_LOG:-}"

log_event() {
    if [ -n "$log_path" ]; then
        printf '%s\n' "$1" >> "$log_path"
    fi
}

installed_url=""
installed_ref=""
if [ -d "$plugin_dir/.git" ]; then
    installed_url="$(git -C "$plugin_dir" config --get remote.origin.url || true)"
    installed_ref="$(git -C "$plugin_dir" rev-parse HEAD 2>/dev/null || true)"
fi

if [ "$installed_url" = "$desired_url" ] && [ "$installed_ref" = "$desired_ref" ]; then
    log_event "bootstrap-plugin unchanged $plugin_name $desired_ref"
    printf 'mise-skills: plugin %s is already pinned to %s\n' "$plugin_name" "$desired_ref"
    exit 0
fi

if [ -d "$plugin_dir" ]; then
    log_event "bootstrap-plugin replace $plugin_name $installed_ref -> $desired_ref"
    mise plugins uninstall --yes "$plugin_name"
else
    log_event "bootstrap-plugin install $plugin_name $desired_ref"
fi

mise plugins install "$plugin_name" "$desired_url#$desired_ref"
