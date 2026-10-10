#!/bin/sh
# Build Estuary for this machine into target/estuary/current,
# where `cargo test -p magnitide-plugins` and `cargo tauri dev` find it.
# Needs Python 3.14 and uv (https://docs.astral.sh/uv/) on PATH, or set UV.
set -e
cd "$(dirname "$0")/../.."
exec python3 scripts/estuary/build.py --local "$@"
