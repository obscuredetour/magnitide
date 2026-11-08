# Releasing

How a version of Magnitide gets built, signed and published. The
[README](../README.md) says how to build and test it day to day.

1. Set the version in `Cargo.toml` (`[workspace.package]`, which every crate shares) and
   `crates/app/ui/package.json`, and merge that to `main`.
2. Tag it with that version: `git tag v<version> && git push origin v<version>`.
   `.github/workflows/release.yml` builds macOS (Apple Silicon and Intel `.dmg`s), Windows
   (`.msi` and `-setup.exe`) and Linux (`.AppImage`, `.deb`, `.rpm`) into a draft release.
3. Look the draft over and publish it as the latest release: the site's download buttons read
   GitHub's latest release (Estuary's releases are never marked latest), and so does the in-app
   updater, through the release's `latest.json`. Publishing is what offers it to everyone.
4. Update the Arch package: `scripts/aur-update.sh <version>`, commit it to `main`, then push it to the AUR
   ([packaging/aur/README.md](packaging/aur/README.md)). Commit it even while the package isn't on the AUR:
   the README and magnitide.com tell Arch users to build it from `main`.

If a build fails because of the workflow rather than the code (a runner GitHub has retired, say),
fix the workflow on `main` and rebuild without moving the tag: Actions › Release › Run workflow on
`main`, with the tag and, optionally, just the builds that failed (`platforms`, e.g.
`macOS Apple Silicon`). They join the same draft release:

```sh
gh workflow run release.yml --ref main -f tag=v<version> -f platforms="macOS Apple Silicon"
```

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

## Updates

Magnitide updates itself (Settings › About, and a banner when the daily check finds a version) from
the latest release's `latest.json`, which `tauri-action` writes from the builds' updater signatures.
The AppImage, the macOS app, the Windows installers, and the `.deb` and `.rpm` (while dpkg or rpm
owns the program) update this way; the AUR package and source builds are left to their package
manager (`crates/app/src/updates.rs`).

The installers are signed with an updater key, separate from Apple's, and the release fails
without it:

| Secret | What it is |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The private key: the contents of the file `cargo tauri signer generate -w <file>` wrote |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Its password, if it has one |

Its public half is `plugins.updater.pubkey` in `crates/app/tauri.conf.json`. Keep the private key
somewhere safe as well as in the secret: every copy out there accepts only updates signed with it,
so a lost key means people have to download the next version by hand, once, to get one with a new
public key.

To try a signed build locally:

```sh
TAURI_SIGNING_PRIVATE_KEY="$(cat <key file>)" cargo tauri build --config '{"bundle":{"createUpdaterArtifacts":true}}'
```
