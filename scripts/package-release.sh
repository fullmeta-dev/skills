#!/usr/bin/env bash
# Build release assets consumed by mise's GitHub backend.
set -Eeuo pipefail

version="${1:?usage: package-release.sh VERSION OUTPUT_DIR [TARGET...] }"
out_dir_input="${2:?usage: package-release.sh VERSION OUTPUT_DIR [TARGET...] }"
shift 2
targets=("$@")
if ((${#targets[@]} == 0)); then
    targets=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc)
fi
mkdir -p "$out_dir_input"
out_dir="$(cd -- "$out_dir_input" && pwd)"
for target in "${targets[@]}"; do
    cargo build --release --manifest-path skills/mise/mise-skills/Cargo.toml --target "$target"
    binary="skills/mise/mise-skills/target/$target/release/skills"
    if [[ "$target" == *windows* ]]; then
        binary+=".exe"
    fi
    [[ -x "$binary" || -f "$binary" ]] || {
        printf 'missing release binary: %s\n' "$binary" >&2
        exit 1
    }
    if [[ "$target" == *windows* ]]; then
        archive="$out_dir/skills_${version}_${target}.zip"
        (cd "$(dirname "$binary")" && zip -q "$archive" "$(basename "$binary")")
        [[ "$(unzip -Z1 "$archive")" == skills.exe ]] || {
            printf 'invalid Windows package: %s\n' "$archive" >&2
            exit 1
        }
    else
        archive="$out_dir/skills_${version}_${target}.tar.gz"
        tar -C "$(dirname "$binary")" -czf "$archive" "$(basename "$binary")"
        [[ "$(tar -tzf "$archive")" == skills ]] || {
            printf 'invalid Unix package: %s\n' "$archive" >&2
            exit 1
        }
    fi
done
(
    cd "$out_dir"
    sha256sum skills_* > SHA256SUMS
)
printf 'packaged skills %s in %s\n' "$version" "$out_dir"
