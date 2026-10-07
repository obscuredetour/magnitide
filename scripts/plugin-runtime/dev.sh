#!/bin/sh
# Build the plugin runtime for this machine into target/plugin-runtime/current,
# where `cargo test -p magnitide-plugins` and `cargo tauri dev` find it.
# Needs Python 3.14 and uv (https://docs.astral.sh/uv/) on PATH, or set UV.
set -e
cd "$(dirname "$0")/../.."
exec python3 scripts/plugin-runtime/build.py --local "$@"
