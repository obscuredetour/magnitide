#!/bin/sh
# Build a Magnitide plugin written in Rust into a .mgp:
#   scripts/build-plugin.sh plugins/openlibrary-source
# Needs the wasm32-wasip2 target (rustup target add wasm32-wasip2). The
# plugin folder holds Cargo.toml, manifest.toml and src/; the .mgp lands in
# target/plugins/<name>.mgp with the component as plugin.wasm.
set -eu
dir="$1"
cd "$(dirname "$0")/.."
name="$(grep -E '^name *= *"' "$dir/Cargo.toml" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
cargo build --release --target wasm32-wasip2 --manifest-path "$dir/Cargo.toml" --target-dir target/plugins/build
wasm="target/plugins/build/wasm32-wasip2/release/$(echo "$name" | tr '-' '_').wasm"
out="target/plugins/$name.mgp"
mkdir -p target/plugins
rm -f "$out"
tmp="$(mktemp -d)"
cp "$dir/manifest.toml" "$tmp/manifest.toml"
cp "$wasm" "$tmp/plugin.wasm"
[ -d "$dir/resources" ] && cp -R "$dir/resources" "$tmp/resources"
(cd "$tmp" && zip -q -r "$OLDPWD/$out" .)
rm -rf "$tmp"
echo "$out ($(du -h "$out" | cut -f1))"
