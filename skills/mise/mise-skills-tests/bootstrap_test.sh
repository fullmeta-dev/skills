#!/usr/bin/env bash
# Regression coverage for the public, ref-pinned bootstrap script.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mise-skills-bootstrap.XXXXXX")"
SERVER_PID=""
cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

REF="0123456789abcdef0123456789abcdef01234567"
SERVER_ROOT="$WORK_DIR/server-root"
CONFIG_DIR="$WORK_DIR/config"
BIN_DIR="$WORK_DIR/bin"
REQUEST_LOG="$WORK_DIR/requests.log"
MISE_LOG="$WORK_DIR/mise.log"
PORT_FILE="$WORK_DIR/port"
mkdir -p "$SERVER_ROOT/$REF/skills/mise" "$SERVER_ROOT/$REF/docs" \
    "$CONFIG_DIR" "$BIN_DIR"

cat > "$SERVER_ROOT/$REF/skills/mise/bootstrap-plugin.sh" <<'EOF'
#!/usr/bin/env bash
printf 'fixture helper\n'
EOF
cat > "$SERVER_ROOT/$REF/docs/mise-skills-bootstrap.toml" <<EOF
# fixture config for $REF
EOF
: > "$REQUEST_LOG"

cat > "$WORK_DIR/server.py" <<'PY'
import http.server
import pathlib
import sys
from urllib.parse import urlparse

root, request_log, port_path = map(pathlib.Path, sys.argv[1:])


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format_string, *args):
        pass

    def do_GET(self):
        path = urlparse(self.path).path
        with request_log.open("a", encoding="utf-8") as log:
            log.write(path + "\n")
        relative = path.removeprefix("/fixture/")
        source = root / relative
        if not source.is_file():
            self.send_error(404)
            return
        body = source.read_bytes()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
port_path.write_text(str(server.server_address[1]), encoding="ascii")
server.serve_forever()
PY
python3 -u "$WORK_DIR/server.py" "$SERVER_ROOT" "$REQUEST_LOG" "$PORT_FILE" \
    >"$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!
for _ in {1..50}; do
    [[ -s "$PORT_FILE" ]] && break
    sleep 0.1
done
[[ -s "$PORT_FILE" ]]
BASE_URL="http://127.0.0.1:$(cat "$PORT_FILE")/fixture"

cat > "$BIN_DIR/mise" <<'EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
printf '%s\n' "$*" > "$MISE_BOOTSTRAP_MISE_LOG"
[[ "$#" -eq 2 && "$1" == bootstrap && "$2" == --yes ]]
EOF
chmod 755 "$BIN_DIR/mise"

export HOME="$WORK_DIR/home"
export MISE_CONFIG_DIR="$CONFIG_DIR"
export MISE_SKILLS_BOOTSTRAP_BASE_URL="$BASE_URL"
export MISE_BOOTSTRAP_MISE_LOG="$MISE_LOG"
export PATH="$BIN_DIR:$PATH"
mkdir -p "$HOME"

bash "$SCRIPT_DIR/../bootstrap.sh" "$REF"

cmp "$SERVER_ROOT/$REF/skills/mise/bootstrap-plugin.sh" \
    "$CONFIG_DIR/mise-skills-bootstrap-plugin.sh"
cmp "$SERVER_ROOT/$REF/docs/mise-skills-bootstrap.toml" \
    "$CONFIG_DIR/config.toml"
[[ -x "$CONFIG_DIR/mise-skills-bootstrap-plugin.sh" ]]
[[ "$(cat "$MISE_LOG")" == 'bootstrap --yes' ]]
grep -Fxq "/fixture/$REF/skills/mise/bootstrap-plugin.sh" "$REQUEST_LOG"
grep -Fxq "/fixture/$REF/docs/mise-skills-bootstrap.toml" "$REQUEST_LOG"
[[ "$(wc -l < "$REQUEST_LOG")" -eq 2 ]]
! grep -Fq '/main/' "$REQUEST_LOG"

printf 'public bootstrap fetched every artifact at supplied ref: %s\n' "$REF"
