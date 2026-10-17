# Magnitide

A fast, modern ebook library manager that starts from the calibre library you already have.
[magnitide.com](https://magnitide.com) · GPL-3.0-or-later ([LICENSE](LICENSE)) · [NOTICE](NOTICE) credits [calibre](https://calibre-ebook.com) and Kovid Goyal

Magnitide opens your calibre library as it is, read-only, so calibre keeps working alongside it. When you're ready
to edit, import it into a Magnitide library of its own: books are copied as copy-on-write clones where the disk
supports it (no extra space on APFS, Btrfs or XFS), and your custom columns come too. Underneath is a native Rust
core with a Tauri desktop app, built to stay quick with 50,000 books and more.

_**This wouldn't exist without [calibre](https://github.com/kovidgoyal/calibre) and the incredible work of [Kovid Goyal](https://github.com/kovidgoyal), all contributors to calibre, and its community.**_

## What it does

- **Browse and find.** Covers or a sortable, resizable table; search with `author:`, `tag:`, `series:` and your own
  fields (`#pages:300`); shelves of your own; a details panel with everything about a book.
- **Edit.** One book or many at once, including author sort names, omnibus editions, several series per book and
  custom fields. Find Metadata looks books up on Open Library and Google Books and lets you take what you want field
  by field. Folders and file names follow your edits, and metadata can be written back into EPUB files.
- **Add books.** Drag and drop EPUB, MOBI, AZW3 and more, with their metadata and covers. Drop a folder and
  its folders are searched; drop a calibre library and it can be imported with its tags, series and ratings.
  Comics (CBZ, CBR, CB7) bring their ComicInfo.xml: issue, format, artists by role, story arcs, characters and covers,
  and CBRs can become CBZs (bsdtar, 7-Zip or unrar reads RAR). Read them in the app: one page or two, right to left
  for manga, picking up where you left off.
- **E-readers.** Kobo, Kindle and other devices show up when plugged in. Books go over in the format each reads
  best, converted from EPUB to KEPUB or AZW3 natively (calibre is only a fallback), and shelves become collections
  on a Kobo, for Kobo's own app and KOReader alike.
- **On your network.** An optional library server with an OPDS catalogue for reading apps, a simple page for
  e-reader browsers, and KOReader progress sync.
- **Magnitide plugins.** Small, sandboxed WebAssembly components (`.mgp` files, added in Settings → Plugins) that
  can do only what their package declares. Four kinds: metadata sources in Find Metadata, import hooks on files being
  added, metadata readers and writers for formats, and library tools in the book menu that work on the selected books,
  with progress in the job bar and questions mid-run. Written in any language with a WebAssembly toolchain; a Rust SDK,
  a template and two sources (Open Library, Goodreads) are in the repository ([docs/plugin-api.md](docs/plugin-api.md)).
- **Covers set the mood.** Open a book and the details panel takes on its cover: the three colours that define
  it, the one it's mostly made of and the two that stand out most against it, laid as a soft gradient behind the
  text, the way a now-playing screen takes on album art. The colours are read from the cover itself, judged by hue
  and saturation rather than brightness, so a black cover with gold lettering gives black and gold, not black and
  grey, and the gilt and ivy on a navy cover both come through. Built for the Light and Dark themes; a setting turns
  it on for every theme.
- **Make it yours.** Themes (light, dark, sepia, Catppuccin, Dracula and more), accent colours, and interface size.

### Calibre Plugins Support

The calibre plugins you rely on run unchanged, in whichever host you choose in Settings → Plugins:

- **Your already installed calibre**
- **A calibre (latest) Magnitide downloads for you, unpacked and used only by Magnitide**
- **(Experimental) Estuary, Magnitide's compatibility layer for calibre plugins, which needs no calibre at all. This is still under development and may not support all plugins.**

Metadata sources join Find Metadata. Toolbar plugins work on the selected books from the Plugins button or the book menu (right click); their windows open as in calibre, and what they change comes back into the library through Magnitide's own editing.
File-type plugins run when books are added, metadata readers and writers handle their formats, and plugins' own settings open from Settings. Conversion, catalog, device, store, viewer and editor plugins aren't supported ([docs/calibre-plugins.md](docs/calibre-plugins.md)).

Magnitide is in active development: the [roadmap](docs/roadmap.md) tracks what's done and what's next.

## Docs

- [docs/architecture.md](docs/architecture.md): how the pieces fit, and the decisions behind them
- [docs/plugin-api.md](docs/plugin-api.md): Magnitide's plugin API, the sandbox, the `.mgp` format, writing a plugin
- [docs/calibre-plugins.md](docs/calibre-plugins.md): calibre plugins: the three hosts, what each kind of plugin gets,
  the compatibility package, Estuary; [coverage](docs/calibre-plugins-coverage.md) of real plugins
- [docs/themes.md](docs/themes.md): themes and add-on theme files
- [docs/roadmap.md](docs/roadmap.md): what's done and what's next
- [docs/releasing.md](docs/releasing.md): versions, tags, the release workflow, macOS signing and the AUR package

## Crates

| Crate                                     | What it is                                                                                                                                                                                                                                                        |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`app`](crates/app)                       | The desktop app: Tauri 2, with a Solid + TypeScript UI in [`crates/app/ui`](crates/app/ui). Browse, search, edit, import and look up metadata                                                                                                                     |
| [`core`](crates/core)                     | `magnitide-core`: the native Magnitide library (schema, migrations, edits), calibre import, adding book files with EPUB metadata. No Tauri code, so the planned server can share it                                                                               |
| [`catalog`](crates/catalog)               | `magnitide-catalog`: the in-memory book model, search and sort that every kind of library loads into                                                                                                                                                              |
| [`sources`](crates/sources)               | `magnitide-sources`: metadata lookup from Open Library and Google Books, on request only                                                                                                                                                                          |
| [`plugins`](crates/plugins)               | `magnitide-plugins`: runs calibre plugins through one of three hosts: the user's installed calibre, a calibre Magnitide downloads, or Estuary, Magnitide's compatibility layer (CPython + PyQt6, downloaded once, with its own `calibre` compatibility package in `python/`) |
| [`plugins-wasm`](crates/plugins-wasm)     | `magnitide-plugins-wasm`: runs Magnitide's own plugins, WebAssembly components on the `magnitide:plugin` API, sandboxed in wasmtime ([docs/plugin-api.md](docs/plugin-api.md))                                                                                    |
| [`plugin-sdk`](crates/plugin-sdk)         | `magnitide-plugin-sdk`: the API's WIT definitions and the Rust bindings for writing a plugin; [`plugins/`](plugins) holds the template, the Open Library example and the Goodreads port                                                                           |
| [`server`](crates/server)                 | `magnitide-server`: serves a library read-only on the local network, as a plain web page for e-reader browsers and an OPDS catalogue for reading apps                                                                                                             |
| [`calibre-compat`](crates/calibre-compat) | Read-only access to calibre libraries (`metadata.db` + book folders), plus a CLI to inspect them                                                                                                                                                                  |

Beside the crates: [`plugins/`](plugins) holds Magnitide plugins written in Rust (the template, the Open Library
example and the Goodreads port, each its own crate); [`scripts/`](scripts) holds `new-plugin.sh` and `build-plugin.sh`
for them, [`scripts/estuary/`](scripts/estuary) builds and fetches Estuary, which CI publishes
as `estuary-v<N>` releases, and [`scripts/calibre-release/`](scripts/calibre-release) pins the calibre release
Magnitide can download.

## Setup

Needs Rust with the `wasm32-wasip2` target (the plugin tests build a WebAssembly fixture), Node with
[pnpm](https://pnpm.io), and the Tauri CLI:

```bash
rustup target add wasm32-wasip2
cargo install tauri-cli --version "^2" --locked
pnpm --dir crates/app/ui install
```

The calibre plugin host tests need a host: `scripts/estuary/fetch.sh` downloads the published Estuary
into `target/estuary/current` (they're skipped without one), and `MAGNITIDE_PLUGIN_HOST=calibre` runs them in
an installed calibre instead. Details in [docs/calibre-plugins.md](docs/calibre-plugins.md#developing).

### Supported systems

macOS 13 or later, Windows 10 or later (with the WebView2 runtime, built in to
Windows 11), and Linux with WebKitGTK 4.1 (Ubuntu 22.04 or later). The floor is
the web engine: the UI needs what Safari 16.4 introduced (Tailwind CSS v4), so
`minimumSystemVersion` in `tauri.conf.json` and `build.target` in
`vite.config.ts` are set to match.

Every push to `main` and every pull request runs the checks in
`.github/workflows/checks.yml` (Rust format, clippy and tests; the UI's types,
formatting and build) on Linux.

## Running

```bash
cd crates/app
cargo tauri dev                  # live reload: UI changes appear on save, Rust changes rebuild
cargo tauri build --bundles app  # release Magnitide.app in target/release/bundle/macos/
```

It opens the library you used last, or else the one calibre last used, falling
back to `~/Calibre Library`. Click the library name in the top-left to open a
different one. calibre libraries are only ever read; Magnitide libraries are
yours to edit. Cover thumbnails are cached in the app's cache folder
(`~/Library/Caches/com.magnitide.app` on macOS).

Keys: `/` or ⌘F search · ⌘, settings · ⌘1 covers, ⌘2 list · arrows move · ⌘-click / Shift-click / ⌘A select several · Enter opens details · ⌘E edits · double-click or ⌘O opens the book in its default app · Esc closes.
In the details panel, click a format to open that specific file, or its × to remove that format (the file goes to the Trash). Right-click a book (or a selection) for everything you can do with it.
Search accepts `author:`, `tag:`, `series:`, `title:`, `rating:`, `#custom:`, quotes, `-` to exclude, `=` for a whole value (`tag:=Fantasy`) and an empty value for "any" (`tags:`, `-tags:`).

## Development

```bash
cargo test                             # all crates; also regenerates the UI's TypeScript bindings
pnpm --dir crates/app/ui lint          # typecheck + formatting (`format` to fix)
cargo run -p calibre-compat -- info    # CLI against the default library
```

Plugins:

```bash
scripts/new-plugin.sh plugins/my-source "My Source" metadata-source   # a plugin from the template (any kind)
scripts/build-plugin.sh plugins/my-source                             # → target/plugins/my-source.mgp
cargo run -p magnitide-plugins-wasm --bin mgp -- check target/plugins/my-source.mgp   # or identify, read, hook
```

Styling is [Tailwind v4](https://tailwindcss.com) utilities in the components. Icons are [Lucide](https://lucide.dev), always through `components/Icon.tsx`. Colours
and shadows come only from the theme (`crates/app/ui/src/themes/`, starting at `index.css`), in two layers:
a palette named after [Catppuccin's](https://github.com/catppuccin/catppuccin/blob/main/docs/style-guide.md)
(crust, mantle, base, surface 0–2, overlay 0–2, subtext 0–1, text, and the accents),
and roles the UI paints with (`--bg`, `--panel`, `--popover`, `--field`, `--element`,
`--fg`, `--link`, `--accent`, `--hover`, `--selection`, `--highlight`, `--row-stripe`,
`--focus`, status colours, `--inverse`, `--scrim`…), derived from the palette following
the Catppuccin and [Dracula](https://draculatheme.com/spec) role tables. A theme is a
palette plus any role overrides; `theme.ts` switches between them at runtime by setting
`data-theme` (and `data-appearance`) on `<html>`. Built in: light, dark, sepia, the four
Catppuccin flavours, Dracula and Alucard, and "system" (follows the OS). An accent picker
(theme menu, Settings → Themes) swaps in any of the theme's palette accents, as
Catppuccin's ports do. The webview's focus ring, text cursor and form controls follow the
theme too. To add a theme, add a block in a `themes/` family file (a Catppuccin port pastes
its palette as is) and register it in `BUILTIN_THEMES` in `theme.ts` with its appearance and
accents. Add-on themes are JSON theme files (palette plus role overrides, checked in Rust,
never raw CSS) in the app's themes folder, or a Catppuccin-style `palette.json`; see
[docs/themes.md](docs/themes.md).

The types the UI uses for Rust commands (`crates/app/ui/src/bindings/`) are
generated from the Rust structs by [ts-rs](https://github.com/Aleph-Alpha/ts-rs)
when tests run. After changing a command's types, run `cargo test` and commit
the regenerated files; `crates/app/ui/src/api.ts` wraps each command.

Releasing a version (the tag, the release workflow, macOS signing, the AUR package):
[docs/releasing.md](docs/releasing.md).

## Roadmap

What's done and what's next, item by item: [docs/roadmap.md](docs/roadmap.md). Open as of this version: a PDF
metadata reader, MOBI/AZW3 → EPUB, Kindle cover thumbnails, MTP devices, Find Duplicates, export to a calibre library,
reading positions in the app, a headless server with accounts and Kobo sync (and plugins answering by policy there),
and a public home for the plugin SDK.

## Why

This began as a fun design exercise: what would I want an ebook manager to look like? Then it became unhinged. Yes, this whole project is a bit unhinged. "What would it look like?" turned into "Can I actually build this?", and every answer pushed the idea further until it grew into Magnitide.

So, honestly, Magnitide exists for me and because I wanted to see if I could do it. But it's also here to offer everything great about calibre with a modern interface, a consistent theming system and a better experience overall.
