#!/usr/bin/env bash
# Build release assets consumed by mise's GitHub backend.
set -Eeuo pipefail

version="${1:?usage: package-release.sh VERSION OUTPUT_DIR [TARGET...] }"
out_dir_input="${2:?usage: package-release.sh VERSION OUTPUT_DIR [TARGET...] }"
shift 2
targets=("$@")
if ((${#targets[@]} == 0)); then
    targets=(x86_64-unknown-linux-gnu aarch64-apple-darwin)
fi
mkdir -p "$out_dir_input"
out_dir="$(cd -- "$out_dir_input" && pwd)"
for target in "${targets[@]}"; do
    case "$target" in
        x86_64-unknown-linux-gnu|aarch64-apple-darwin) ;;
        *)
            printf 'unsupported release target: %s (supported: x86_64-unknown-linux-gnu, aarch64-apple-darwin)\n' "$target" >&2
            exit 1
            ;;
    esac
    cargo build --release --manifest-path packages/skills/Cargo.toml --target "$target"
    binary="packages/skills/target/$target/release/skills"
    [[ -x "$binary" || -f "$binary" ]] || {
        printf 'missing release binary: %s\n' "$binary" >&2
        exit 1
    }
    archive="$out_dir/skills_${version}_${target}.tar.gz"
    tar -C "$(dirname "$binary")" -czf "$archive" "$(basename "$binary")"
    [[ "$(tar -tzf "$archive")" == skills ]] || {
        printf 'invalid Unix package: %s\n' "$archive" >&2
        exit 1
    }
done
(
    cd "$out_dir"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum skills_* > SHA256SUMS
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 skills_* > SHA256SUMS
    else
        printf 'no SHA-256 checksum utility found (sha256sum or shasum)\n' >&2
        exit 1
    fi
)
printf 'packaged skills %s in %s\n' "$version" "$out_dir"
