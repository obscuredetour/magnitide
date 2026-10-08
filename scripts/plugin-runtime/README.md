# The plugin runtime

calibre plugins run in a Python that Magnitide downloads once, not in an
installed calibre: CPython (from python-build-standalone) with PyQt6, lxml,
Pillow and the other wheels in `requirements.in`, stripped to what plugins
use. `build.py` assembles it for one target and either leaves it in
`target/plugin-runtime/` for development or packs it into the archive that
Magnitide downloads (`magnitide-plugin-runtime-<VERSION>-<target>.tar.zst`).

The calibre compatibility package (`crates/plugins/python/`) is *not* in the
archive: it ships inside Magnitide and versions with it. The archive holds
only the interpreter and wheels, which change rarely.

## Files

| File | What it is |
|---|---|
| `VERSION` | The runtime's version, an integer. Bump it when `python.json` or `requirements.txt` change. Must match `runtime::VERSION` in `crates/plugins/src/runtime.rs`. |
| `python.json` | The python-build-standalone release and the sha256 of its tarball per target. |
| `requirements.in` | The wheels, with why each is there. |
| `requirements.txt` | The lockfile: every platform's wheels with hashes. Regenerate with `uv pip compile requirements.in --universal --python-version 3.14 --generate-hashes --only-binary :all: --no-header -o requirements.txt`. |
| `sha256sums.txt` | The published archives' hashes, written by `build.py --manifest`; `runtime::RELEASES` must match it (a unit test checks). |
| `build.py` | The build. Python 3.14 and `uv` on PATH (or `UV=/path/to/uv`). |
| `dev.sh` | `build.py --local` for this machine. |
| `fetch.sh` | Downloads a released runtime for this machine (or a target given) into `target/plugin-runtime/`, checked against `sha256sums.txt`; what CI uses. |

## Developing

```sh
scripts/plugin-runtime/dev.sh
```

builds the runtime for this machine into `target/plugin-runtime/<target>/`
and points `target/plugin-runtime/current` at it. The tests and the app find
it there; `MAGNITIDE_PLUGIN_RUNTIME=<dir containing python/>` overrides, and
`MAGNITIDE_COMPAT_DIR=crates/plugins/python` makes the app use the compat
package from the source tree instead of the copy embedded in the binary.

## Releasing

`.github/workflows/plugin-runtime.yml` builds every target, runs each
build's smoke test on its own platform, and attaches the archives to the
GitHub release `plugin-runtime-v<VERSION>` together with `manifest.json`.
Then `python3 scripts/plugin-runtime/build.py --rust-table manifest.json`
prints the `RELEASES` table for `runtime.rs`; paste it in and commit
`sha256sums.txt`. Magnitide only installs an archive whose hash is compiled
into it.

## Linux

Qt's xcb platform plugin needs system libraries most desktops already have:
libxcb (with cursor, icccm, image, keysyms, randr, render-util, shape, sync,
xfixes, xinerama, xkb), libxkbcommon and libxkbcommon-x11, libGL/libEGL,
fontconfig, freetype, dbus, glib, and the Wayland client libraries for
Wayland sessions. `python -I -m magnitide_host doctor` reports what's
missing; Settings › Plugins shows it.
