#!/bin/sh
# Point packaging/aur/magnitide-bin at a release: its version, pkgrel 1, and
# the sha256 of its .deb (downloaded from the GitHub release), in PKGBUILD and
# .SRCINFO alike. Then copy both into your clone of the AUR repository and
# push (packaging/aur/README.md).
#
#   scripts/aur-update.sh 0.8.0
set -eu
cd "$(dirname "$0")/.."
version="${1:?usage: scripts/aur-update.sh <version>}"
dir=packaging/aur/magnitide-bin
url="https://github.com/obscuredetour/magnitide/releases/download/v$version/Magnitide_${version}_amd64.deb"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "aur-update: downloading $url"
curl -fsSL -o "$tmp/release.deb" "$url"
if command -v sha256sum >/dev/null; then
  sum="$(sha256sum "$tmp/release.deb" | cut -d' ' -f1)"
else
  sum="$(shasum -a 256 "$tmp/release.deb" | cut -d' ' -f1)"
fi

# The same edits on both files (sed -i differs between BSD and GNU, so via a temporary file).
edit() {
  sed -e "s/^pkgver=.*/pkgver=$version/" \
      -e "s/^pkgrel=.*/pkgrel=1/" \
      -e "s/^sha256sums=.*/sha256sums=('$sum')/" \
      -e "s/^\(	pkgver = \).*/\1$version/" \
      -e "s/^\(	pkgrel = \).*/\11/" \
      -e "s|^\(	source = \).*|\1$url|" \
      -e "s/^\(	sha256sums = \).*/\1$sum/" \
      "$1" > "$tmp/edited" && cat "$tmp/edited" > "$1"
}
edit "$dir/PKGBUILD"
edit "$dir/.SRCINFO"
echo "aur-update: $dir is $version ($sum)"
