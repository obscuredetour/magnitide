#!/bin/sh
# Download the published Estuary for this machine (or $1, a target
# triple) into target/estuary/<target> and point
# target/estuary/current at it, as scripts/estuary/dev.sh does
# for a local build. Used by CI; handy when uv and Python 3.14 aren't around.
#
# The archive comes from the GitHub release estuary-v<VERSION> and
# must match scripts/estuary/sha256sums.txt, written when the release
# was made. Exits 2 when no release is published yet (sha256sums.txt
# missing or without this target), so callers can skip the host tests.
set -eu
cd "$(dirname "$0")/../.."
version="$(tr -d '[:space:]' < scripts/estuary/VERSION)"
target="${1:-}"
if [ -z "$target" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux-aarch64) target=aarch64-unknown-linux-gnu ;;
    *) echo "fetch.sh: no Estuary for $(uname -s)-$(uname -m)"; exit 2 ;;
  esac
fi
file="magnitide-estuary-$version-$target.tar.zst"
sums=scripts/estuary/sha256sums.txt
if [ ! -f "$sums" ]; then
  echo "fetch.sh: $sums doesn't exist: no Estuary has been released yet"
  exit 2
fi
expected="$(grep " $file\$" "$sums" | cut -d' ' -f1 || true)"
if [ -z "$expected" ]; then
  echo "fetch.sh: $file isn't in $sums"
  exit 2
fi
base="${MAGNITIDE_ESTUARY_URL:-https://github.com/obscuredetour/magnitide/releases/download/estuary-v$version}"
dest="target/estuary/$target"
cache="${MAGNITIDE_ESTUARY_CACHE:-target/estuary/downloads}"
mkdir -p "$cache" target/estuary
if [ -f "$dest/python/magnitide-estuary.json" ] && [ "$(cat "$dest/.sha256" 2>/dev/null)" = "$expected" ]; then
  echo "fetch.sh: $target Estuary $version already in $dest"
else
  if [ ! -f "$cache/$file" ] || [ "$(shasum -a 256 "$cache/$file" | cut -d' ' -f1)" != "$expected" ]; then
    rm -f "$cache/$file"
    # While the repository is private the assets need a GitHub token: the
    # gh CLI (GH_TOKEN on CI, a login elsewhere) gets them; curl otherwise.
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
      echo "fetch.sh: downloading $file from the estuary-v$version release (gh)"
      gh release download "estuary-v$version" --repo "${MAGNITIDE_REPO:-obscuredetour/magnitide}" \
        --pattern "$file" --dir "$cache" --clobber
    else
      echo "fetch.sh: downloading $base/$file"
      curl -fSL --retry 3 -o "$cache/$file.part" "$base/$file"
      mv "$cache/$file.part" "$cache/$file"
    fi
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
rm -f target/estuary/current
ln -s "$target" target/estuary/current
echo "fetch.sh: target/estuary/current -> $target"
