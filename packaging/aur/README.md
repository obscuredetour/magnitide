# Arch Linux (AUR)

`magnitide-bin/` is the [AUR](https://aur.archlinux.org) package `magnitide-bin`: each release's `.deb`
from GitHub, repackaged (no build), so Arch users can `yay -S magnitide-bin` (or `paru`, or
`makepkg -si` by hand). The AUR keeps its own git repository per package; these files are its source.

Installed this way, Magnitide leaves updates to pacman and the AUR helper: its own updater (for the
AppImage, `.deb` and `.rpm`) sees that dpkg doesn't own the program, and Settings › About says so.

## Publishing it the first time

1. Make an account at aur.archlinux.org and add an SSH public key to it (My Account › SSH Public Key).
2. Clone the package's repository (empty until the first push; cloning a new name creates it):
   ```sh
   git clone ssh://aur@aur.archlinux.org/magnitide-bin.git
   ```
3. Copy `PKGBUILD` and `.SRCINFO` into it, then commit and push:
   ```sh
   cp packaging/aur/magnitide-bin/{PKGBUILD,.SRCINFO} ../magnitide-bin/
   cd ../magnitide-bin && git add PKGBUILD .SRCINFO && git commit -m "magnitide-bin 0.7.0" && git push
   ```

## Each release

```sh
scripts/aur-update.sh 0.8.0
```

sets the version and the `.deb`'s checksum in both files (after the release is published, so the file
can be downloaded). Commit them here, then copy them to the AUR clone and push as above.

On Arch, `makepkg --printsrcinfo > .SRCINFO` regenerates `.SRCINFO` from the `PKGBUILD`, and
`namcap PKGBUILD` and `makepkg -si` check and install it; do that for any change beyond a new
version.
