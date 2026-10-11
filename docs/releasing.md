# Releasing

How a version of Magnitide gets built, signed and published. The
[README](../README.md) says how to build and test it day to day.

1. Set the version in `Cargo.toml` (`[workspace.package]`, which every crate shares) and
   `crates/app/ui/package.json`, and merge that to `main`.
2. Tag it with that version: `git tag v<version> && git push origin v<version>`.
   `.github/workflows/release.yml` builds macOS (Apple Silicon and Intel `.dmg`s), Windows
   (`.msi` and `-setup.exe`) and Linux (`.AppImage`, `.deb`, `.rpm`) into a draft release.
3. Look the draft over and publish it as the latest release: the site's download buttons read
   GitHub's latest release (Estuary's releases are never marked latest).
4. Update the Arch package: `scripts/aur-update.sh <version>`, then push it to the AUR
   ([packaging/aur/README.md](packaging/aur/README.md)).

The macOS builds are signed with a Developer ID and notarized when these repository secrets are
set (Settings › Secrets and variables › Actions); they need a membership of the Apple Developer
Program:

| Secret | What it is |
| --- | --- |
| `APPLE_CERTIFICATE` | Your Developer ID Application certificate with its private key, exported from Keychain Access as a `.p12`, then `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The password you gave the `.p12` |
| `APPLE_SIGNING_IDENTITY` | The certificate's name: `Developer ID Application: <name> (<team id>)` (`security find-identity -v -p codesigning`) |
| `APPLE_ID` | The Apple Account that notarizes |
| `APPLE_PASSWORD` | An app-specific password for that account (account.apple.com › Sign-In and Security) |
| `APPLE_TEAM_ID` | The team id (developer.apple.com › Account › Membership details) |

Without them the Mac builds are ad-hoc signed: they run, but Gatekeeper asks once (System Settings ›
Privacy & Security › Open Anyway), and the release notes say so. Some but not all of them set is an
error. Signed builds run under the hardened runtime; `crates/app/Entitlements.plist` lets wasmtime
compile Magnitide plugins there.
