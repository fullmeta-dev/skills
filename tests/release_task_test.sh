#!/usr/bin/env bash
# Fake-command coverage for the release task. This never contacts GitHub.
set -Eeuo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
task="$root/mise/tasks/release"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/skills-release-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

help_log="$tmp/help.log"
: > "$help_log"
if ! MISE_TASK_NAME=release FAKE_LOG="$help_log" bash "$task" --help >/tmp/skills-release-help.$$ 2>&1; then
    cat /tmp/skills-release-help.$$
    exit 1
fi
rm -f /tmp/skills-release-help.$$
test ! -s "$help_log"

if MISE_TASK_NAME=release FAKE_LOG="$help_log" bash "$task" --patch --minor >/tmp/skills-release-invalid.$$ 2>&1; then
    cat /tmp/skills-release-invalid.$$
    exit 1
fi
rm -f /tmp/skills-release-invalid.$$
test ! -s "$help_log"

origin="$tmp/origin.git"
repo="$tmp/repo"
mkdir -p "$repo/packages/skills" "$tmp/bin"
git init --quiet --bare "$origin"
git init --quiet "$repo"
git -C "$repo" config user.name release-test
git -C "$repo" config user.email release-test@example.invalid
git -C "$repo" branch -M main
cp "$task" "$repo/release-task"
cat > "$repo/packages/skills/Cargo.toml" <<'EOF'
[package]
name = "skills"
version = "0.1.0"
edition = "2021"
EOF
cat > "$repo/packages/skills/Cargo.lock" <<'EOF'
[[package]]
name = "skills"
version = "0.1.0"
EOF
git -C "$repo" add .
git -C "$repo" commit --quiet -m initial
git -C "$repo" remote add origin "$origin"
git -C "$repo" push --quiet -u origin main

cat > "$tmp/bin/git" <<'EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
printf 'git %s\n' "$*" >> "${FAKE_LOG:?}"
if [[ "${1:-}" == push ]]; then
    exit 0
fi
exec /usr/bin/git "$@"
EOF
cat > "$tmp/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
printf 'cargo %s\n' "$*" >> "${FAKE_LOG:?}"
if [[ "${1:-}" == check ]]; then
    /usr/bin/python3 - "$PWD/packages/skills/Cargo.toml" "$PWD/packages/skills/Cargo.lock" <<'PY'
import re
import sys
from pathlib import Path
manifest, lock = map(Path, sys.argv[1:])
text = manifest.read_text()
version = re.search(r'^version = "([^"]+)"$', text, re.M).group(1)
lock_text = lock.read_text()
lock.write_text(re.sub(r'(name = "skills"\nversion = ")([^"]+)(")', rf'\g<1>{version}\3', lock_text, count=1))
PY
fi
EOF
cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
printf 'gh %s\n' "$*" >> "${FAKE_LOG:?}"
case "${1:-}" in
  auth) exit 0 ;;
  workflow) exit 0 ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/git" "$tmp/bin/cargo" "$tmp/bin/gh"

initial_log="$tmp/initial-commands.log"
before_initial="$(sha256sum "$repo/packages/skills/Cargo.toml" "$repo/packages/skills/Cargo.lock")"
initial_head="$(/usr/bin/git -C "$repo" rev-parse HEAD)"
PATH="$tmp/bin:/usr/bin:/bin" \
FAKE_LOG="$initial_log" \
MISE_PROJECT_ROOT="$repo" \
bash "$task" --version 0.1.0 --yes

test "$before_initial" = "$(sha256sum "$repo/packages/skills/Cargo.toml" "$repo/packages/skills/Cargo.lock")"
test "$initial_head" = "$(/usr/bin/git -C "$repo" rev-parse refs/tags/v0.1.0^{commit})"
grep -F 'git tag -a v0.1.0 -m Release skills v0.1.0' "$initial_log"
grep -F 'git push origin main' "$initial_log"
grep -F 'git push origin v0.1.0' "$initial_log"
test "$(grep -c '^git commit' "$initial_log" || true)" = 0
test "$(grep -c '^cargo ' "$initial_log" || true)" = 0
test "$(grep -c '^gh workflow run' "$initial_log")" = 1

initial_repeat_output="$tmp/initial-repeat.out"
if PATH="$tmp/bin:/usr/bin:/bin" \
    FAKE_LOG="$tmp/initial-repeat.log" \
    MISE_PROJECT_ROOT="$repo" \
    bash "$task" --version 0.1.0 --yes >"$initial_repeat_output" 2>&1; then
    cat "$initial_repeat_output"
    exit 1
fi
grep -F 'release: tag v0.1.0 already exists locally or remotely' "$initial_repeat_output"

log="$tmp/commands.log"
: > "$log"
PATH="$tmp/bin:/usr/bin:/bin" \
FAKE_LOG="$log" \
MISE_PROJECT_ROOT="$repo" \
bash "$task" --version v0.1.1 --yes

grep -F 'cargo check --manifest-path' "$log"
grep -F 'cargo test --manifest-path' "$log"
grep -F 'git commit -m Release skills v0.1.1' "$log"
grep -F 'git tag -a v0.1.1 -m Release skills v0.1.1' "$log"
grep -F 'git push origin main' "$log"
grep -F 'git push origin v0.1.1' "$log"
grep -F 'gh workflow run release.yml --repo fullmeta-dev/skills --ref v0.1.1' "$log"
test "$(grep -c '^gh workflow run' "$log")" = 1
/usr/bin/git -C "$repo" push --quiet origin main
# A bare invocation defaults to a patch bump. The two prompts are independent:
# accept Git changes, decline dispatch.
printf 'y\nn\n' | PATH="$tmp/bin:/usr/bin:/bin" \
FAKE_LOG="$log" \
MISE_PROJECT_ROOT="$repo" \
bash "$task"
test "$(grep -c '^gh workflow run' "$log")" = 1
test "$(grep -c '^git push' "$log")" = 4
# Establish the interactive tag in the local fixture's remote, then verify resume
# performs only the external dispatch and does not push or rewrite files.
/usr/bin/git -C "$repo" push --quiet origin main v0.1.2
before_resume="$(sha256sum "$repo/packages/skills/Cargo.toml" "$repo/packages/skills/Cargo.lock")"
PATH="$tmp/bin:/usr/bin:/bin" \
FAKE_LOG="$log" \
MISE_PROJECT_ROOT="$repo" \
bash "$task" --resume --version v0.1.2 --yes
[ "$before_resume" = "$(sha256sum "$repo/packages/skills/Cargo.toml" "$repo/packages/skills/Cargo.lock")" ]
test "$(grep -c '^gh workflow run' "$log")" = 2
test "$(grep -c '^git push' "$log")" = 4
printf 'release task fake-command tests passed\n'
