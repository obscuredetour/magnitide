#!/bin/sh
# Start a Magnitide plugin from the template:
#   scripts/new-plugin.sh plugins/my-source "My Source" metadata-source
# Kinds: metadata-source (default), import-hook, file-metadata,
# library-action. The folder is created; build it with
# scripts/build-plugin.sh.
set -eu
dir="${1:?a folder for the plugin}"
name="${2:-My Plugin}"
kind="${3:-metadata-source}"
cd "$(dirname "$0")/.."
[ -f "plugins/template/kinds/$kind.rs" ] || { echo "unknown kind $kind" >&2; exit 1; }
[ ! -e "$dir" ] || { echo "$dir exists" >&2; exit 1; }
crate="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-|-$//g')"
mkdir -p "$dir"
cp -R plugins/template/. "$dir"
# A new plugin starts from one kind; the other skeletons stay behind.
rm -r "${dir:?}/kinds"
cp "plugins/template/kinds/$kind.rs" "$dir/src/lib.rs"
# The SDK's path, relative to the new folder.
depth="$(printf '%s' "$dir" | tr -cd '/' | wc -c | tr -d ' ')"
up=""; i=0; while [ "$i" -le "$depth" ]; do up="../$up"; i=$((i + 1)); done
sed -i.bak -E "s|^name = \"my-plugin\"|name = \"$crate\"|; s|path = \"../../crates/plugin-sdk\"|path = \"${up}crates/plugin-sdk\"|" "$dir/Cargo.toml"
caps='[]'; [ "$kind" = metadata-source ] && caps='["network"]'
sed -i.bak -E "s|^name = \"My Plugin\"|name = \"$name\"|; s|^kind = .*|kind = \"$kind\"|; s|^capabilities = .*|capabilities = $caps|" "$dir/manifest.toml"
sed -i.bak -E "s|^# My Plugin|# $name|" "$dir/README.md"
rm -f "${dir:?}/Cargo.toml.bak" "${dir:?}/manifest.toml.bak" "${dir:?}/README.md.bak"
echo "$dir: a $kind plugin named \"$name\" (crate $crate). Build it with scripts/build-plugin.sh $dir"
