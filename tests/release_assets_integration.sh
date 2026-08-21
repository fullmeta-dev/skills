#!/usr/bin/env bash
# Read-only release integration check. It is opt-in so normal test runs never
# contact GitHub, and it becomes meaningful after the first public release.
set -Eeuo pipefail

if [[ "${RUN_RELEASE_INTEGRATION:-0}" != "1" ]]; then
    echo "release asset integration skipped (set RUN_RELEASE_INTEGRATION=1)"
    exit 0
fi

api="${SKILLS_RELEASE_API_URL:-https://api.github.com/repos/fullmeta-dev/skills/releases/latest}"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/skills-release-assets.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
curl --fail --location --silent --show-error \
    --header 'Accept: application/vnd.github+json' \
    --header 'X-GitHub-Api-Version: 2022-11-28' \
    "$api" > "$tmp/release.json"

python3 - "$tmp/release.json" "$tmp/asset-map.json" <<'PY'
import json
import sys
from pathlib import Path

release = json.loads(Path(sys.argv[1]).read_text())
assets = {asset['name']: asset['browser_download_url'] for asset in release['assets']}
required = 'release-compatibility.json'
if required not in assets:
    raise SystemExit('latest release has no release-compatibility.json asset')
Path(sys.argv[2]).write_text(json.dumps(assets), encoding='utf-8')
PY

manifest_url="$(python3 - "$tmp/asset-map.json" <<'PY'
import json
import sys
print(json.load(open(sys.argv[1]))['release-compatibility.json'])
PY
)"
curl --fail --location --silent --show-error "$manifest_url" > "$tmp/release-compatibility.json"
version="$(python3 - "$tmp/release-compatibility.json" <<'PY'
import json
import sys
manifest = json.load(open(sys.argv[1]))
version = manifest['latest']
release = manifest['releases'][version]
assert len(release['plugin_revision']) == 40
print(version)
PY
)"

python3 - "$tmp/release.json" "$tmp/asset-map.json" "$version" <<'PY'
import json
import sys

release = json.load(open(sys.argv[1]))
assets = json.load(open(sys.argv[2]))
version = sys.argv[3]
assert release['tag_name'] == f'v{version}'
expected = {
    f'skills_{version}_x86_64-unknown-linux-gnu.tar.gz',
    f'skills_{version}_aarch64-apple-darwin.tar.gz',
    'SHA256SUMS',
    'release-compatibility.json',
}
missing = expected - assets.keys()
assert not missing, f'missing release assets: {sorted(missing)}'
PY

while IFS= read -r asset; do
    url="$(python3 - "$tmp/asset-map.json" "$asset" <<'PY'
import json
import sys
print(json.load(open(sys.argv[1]))[sys.argv[2]])
PY
)"
    curl --fail --location --silent --show-error "$url" -o "$tmp/$asset"
done < <(printf 'SHA256SUMS\nskills_%s_x86_64-unknown-linux-gnu.tar.gz\nskills_%s_aarch64-apple-darwin.tar.gz\n' "$version" "$version")
(cd "$tmp" && sha256sum -c SHA256SUMS)
printf 'release asset integration passed for v%s\n' "$version"
