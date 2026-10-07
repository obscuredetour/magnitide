#!/bin/sh
# Download the published plugin runtime for this machine (or $1, a target
# triple) into target/plugin-runtime/<target> and point
# target/plugin-runtime/current at it, as scripts/plugin-runtime/dev.sh does
# for a local build. Used by CI; handy when uv and Python 3.14 aren't around.
#
# The archive comes from the GitHub release plugin-runtime-v<VERSION> and
# must match scripts/plugin-runtime/sha256sums.txt, written when the release
# was made. Exits 2 when no release is published yet (sha256sums.txt
# missing or without this target), so callers can skip the host tests.
set -eu
cd "$(dirname "$0")/../.."
version="$(tr -d '[:space:]' < scripts/plugin-runtime/VERSION)"
target="${1:-}"
if [ -z "$target" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux-aarch64) target=aarch64-unknown-linux-gnu ;;
    *) echo "fetch.sh: no runtime for $(uname -s)-$(uname -m)"; exit 2 ;;
  esac
fi
file="magnitide-plugin-runtime-$version-$target.tar.zst"
sums=scripts/plugin-runtime/sha256sums.txt
if [ ! -f "$sums" ]; then
  echo "fetch.sh: $sums doesn't exist: no plugin runtime has been released yet"
  exit 2
fi
expected="$(grep " $file\$" "$sums" | cut -d' ' -f1 || true)"
if [ -z "$expected" ]; then
  echo "fetch.sh: $file isn't in $sums"
  exit 2
fi
base="${MAGNITIDE_PLUGIN_RUNTIME_URL:-https://github.com/obscuredetour/magnitide/releases/download/plugin-runtime-v$version}"
dest="target/plugin-runtime/$target"
cache="${MAGNITIDE_PLUGIN_RUNTIME_CACHE:-target/plugin-runtime/downloads}"
mkdir -p "$cache" target/plugin-runtime
if [ -f "$dest/python/magnitide-runtime.json" ] && [ "$(cat "$dest/.sha256" 2>/dev/null)" = "$expected" ]; then
  echo "fetch.sh: $target runtime $version already in $dest"
else
  if [ ! -f "$cache/$file" ] || [ "$(shasum -a 256 "$cache/$file" | cut -d' ' -f1)" != "$expected" ]; then
    echo "fetch.sh: downloading $base/$file"
    curl -fSL --retry 3 -o "$cache/$file.part" "$base/$file"
    mv "$cache/$file.part" "$cache/$file"
  fi
  actual="$(shasum -a 256 "$cache/$file" | cut -d' ' -f1)"
  if [ "$actual" != "$expected" ]; then
    echo "fetch.sh: $file has sha256 $actual, expected $expected"
    rm -f "$cache/$file"
    exit 1
  fi
  rm -rf "$dest.tmp"
  mkdir -p "$dest.tmp"
  zstd -dc "$cache/$file" | tar -x -C "$dest.tmp"
  rm -rf "$dest"
  mv "$dest.tmp" "$dest"
  echo "$expected" > "$dest/.sha256"
fi
rm -f target/plugin-runtime/current
ln -s "$target" target/plugin-runtime/current
echo "fetch.sh: target/plugin-runtime/current -> $target"
