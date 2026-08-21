#!/usr/bin/env bash
# Build release assets consumed by mise's GitHub backend.
set -Eeuo pipefail

version="${1:?usage: package-release.sh VERSION OUTPUT_DIR [TARGET...] }"
out_dir="${2:?usage: package-release.sh VERSION OUTPUT_DIR [TARGET...] }"
shift 2
targets=("$@")
if ((${#targets[@]} == 0)); then
    targets=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc)
fi
mkdir -p "$out_dir"
for target in "${targets[@]}"; do
    cargo build --release --manifest-path skills/mise/mise-skills/Cargo.toml --target "$target"
    binary="skills/mise/mise-skills/target/$target/release/skills"
    suffix="tar.gz"
    if [[ "$target" == *windows* ]]; then
        binary+=".exe"
        suffix="zip"
    fi
    [[ -x "$binary" || -f "$binary" ]] || { printf 'missing release binary: %s\n' "$binary" >&2; exit 1; }
    if [[ "$suffix" == tar.gz ]]; then
        tar -C "$(dirname "$binary")" -czf "$out_dir/skills_${version}_${target}.tar.gz" "$(basename "$binary")"
    else
        (cd "$(dirname "$binary")" && zip -q "$OLDPWD/$out_dir/skills_${version}_${target}.zip" "$(basename "$binary")")
    fi
done
( cd "$out_dir" && sha256sum skills_* > SHA256SUMS )
printf 'packaged skills %s in %s\n' "$version" "$out_dir"
