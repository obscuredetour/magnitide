#!/bin/sh
# Compile the macOS 26 app icon, crates/app/icons/Magnitide.icon (made in
# Icon Composer: the mark's four slabs as layers, on a ground that's light
# in the Default style and dark in Dark), into crates/app/icons/Assets.car, which
# tauri.conf.json bundles beside icon.icns (the fallback for older macOS).
# The compiled file is committed so builds don't need Xcode 26; run this
# after editing the .icon. Needs Xcode 26's actool; the Command Line Tools
# don't have it, so this uses Xcode's even when xcode-select points at them.
set -eu
cd "$(dirname "$0")/../crates/app/icons"
if ! actool --version >/dev/null 2>&1; then
  export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
fi
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
# The flags Tauri's bundler passes when it compiles a .icon itself.
actool Magnitide.icon --compile "$out" \
  --output-format human-readable-text --notices --warnings \
  --output-partial-info-plist "$out/info.plist" \
  --app-icon Magnitide --include-all-app-icons \
  --enable-on-demand-resources NO --development-region en \
  --target-device mac --minimum-deployment-target 26.0 --platform macosx >/dev/null
cp "$out/Assets.car" Assets.car
echo "crates/app/icons/Assets.car updated"
