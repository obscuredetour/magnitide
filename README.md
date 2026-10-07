# Magnitide

A fast, modern ebook library manager that starts from the calibre library you already have.

Magnitide opens your calibre library as it is, read-only, so calibre keeps working alongside it. When you're ready
to edit, import it into a Magnitide library of its own: books are copied as copy-on-write clones where the disk
supports it (no extra space on APFS, Btrfs or XFS), and your custom columns come too. Underneath is a native Rust
core with a Tauri desktop app, built to stay quick with 50,000 books and more.

## What it does

- **Browse and find.** Covers or a sortable, resizable table; search with `author:`, `tag:`, `series:` and your own
  fields (`#pages:300`); shelves of your own; a details panel with everything about a book.
- **Edit.** One book or many at once, including author sort names, omnibus editions, several series per book and
  custom fields. Find Metadata looks books up on Open Library and Google Books and lets you take what you want field
  by field. Folders and file names follow your edits, and metadata can be written back into EPUB files.
- **Add books.** Drag and drop EPUB, MOBI, AZW3 and more, with their metadata and covers.
- **E-readers.** Kobo, Kindle and other devices show up when plugged in. Books go over in the format each reads
  best, converted from EPUB to KEPUB or AZW3 natively (calibre is only a fallback), and shelves become collections
  on a Kobo, for Kobo's own app and KOReader alike.
- **On your network.** An optional library server with an OPDS catalogue for reading apps, a simple page for
  e-reader browsers, and KOReader progress sync.
- **calibre plugins.** The plugins you rely on run in your own installed calibre, unchanged, with their settings
  kept apart from calibre's. Metadata sources join Find Metadata. Toolbar plugins work on the selected books from
  the Plugins button or the book menu; their windows open as in calibre, and what they change comes back into the
  library through Magnitide's own editing. Still in progress: see the roadmap.
- **Make it yours.** Themes (light, dark, sepia, Catppuccin, Dracula and more), accent colours, and interface size.

Magnitide is in active development: the [roadmap](#roadmap) tracks what's done and what's next. How the pieces fit,
and the decisions behind them: [docs/architecture.md](docs/architecture.md).

## Crates

| Crate                                     | What it is                                                                                                                                                                          |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`app`](crates/app)                       | The desktop app: Tauri 2, with a Solid + TypeScript UI in [`crates/app/ui`](crates/app/ui). Browse, search, edit, import and look up metadata                                       |
| [`core`](crates/core)                     | `magnitide-core`: the native Magnitide library (schema, migrations, edits), calibre import, adding book files with EPUB metadata. No Tauri code, so the planned server can share it |
| [`catalog`](crates/catalog)               | `magnitide-catalog`: the in-memory book model, search and sort that every kind of library loads into                                                                                |
| [`sources`](crates/sources)               | `magnitide-sources`: metadata lookup from Open Library and Google Books, on request only                                                                                            |
| [`plugins`](crates/plugins)               | `magnitide-plugins`: runs calibre plugins in the user's installed calibre (metadata sources, and GUI plugins on a shadow library)                                                   |
| [`server`](crates/server)                 | `magnitide-server`: serves a library read-only on the local network, as a plain web page for e-reader browsers and an OPDS catalogue for reading apps                               |
| [`calibre-compat`](crates/calibre-compat) | Read-only access to calibre libraries (`metadata.db` + book folders), plus a CLI to inspect them                                                                                    |

## Setup

Needs Rust, Node with [pnpm](https://pnpm.io), and the Tauri CLI:

```bash
cargo install tauri-cli --version "^2" --locked
pnpm --dir crates/app/ui install
```

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
(`~/Library/Caches/dev.magnitide.app` on macOS).

Keys: `/` or ⌘F search · ⌘, settings · ⌘1 covers, ⌘2 list · arrows move · ⌘-click / Shift-click / ⌘A select several · Enter opens details · ⌘E edits · double-click or ⌘O opens the book in its default app · Esc closes.
In the details panel, click a format to open that specific file, or its × to remove that format (the file goes to the Trash). Right-click a book (or a selection) for everything you can do with it.
Search accepts `author:`, `tag:`, `series:`, `title:`, `#custom:`, quotes and `-` to exclude.

## Development

```bash
cargo test                             # all crates; also regenerates the UI's TypeScript bindings
pnpm --dir crates/app/ui lint                 # typecheck + formatting (`format` to fix)
cargo run -p calibre-compat -- info    # CLI against the default library
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
(theme menu, Settings → Appearance) swaps in any of the theme's palette accents, as
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

## Roadmap

- [x] Read calibre libraries natively (`calibre-compat`)
- [x] Cover grid, search, sort, details panel, open in default app
- [x] Solid + TypeScript UI with types generated from Rust
- [x] Toolbar (library menu, cover size, theme picker) and table view (sortable, resizable, choosable columns)
- [x] Settings panel (⌘,): appearance, library, list columns, thumbnail cache, about
- [x] List view zebra stripes (setting, on by default)
- [x] Theme roles: zebra stripes, hover, selection and highlight are theme variables (`--row-stripe`, `--hover`, `--selection`, `--highlight`), consistent across themes; Catppuccin (all four flavours), Dracula and Alucard built in
- [x] Clear search button (Jamie)
- [x] Resizable details panel: drag its edge (300–680px, always leaving the list 320px), double-click to reset, or focus the edge and use ←/→
- [x] Add slight app blur behind dialogs (every dialog dims and blurs the app behind it, tinted per theme)
- [x] Covers view keeps the selected book where it was on screen when the panel opens or resizes, or the cover size changes
- [x] "Send to device" & "Convert" are real buttons under the formats in the details panel (Send is the panel's main action when a device is connected)
- [x] Table column reordering: drag a header sideways (or ⌥←/⌥→ on a focused one); reset from the ⋯ menu or Settings
- [x] Right-click menu for books (also the keyboard's menu key or Shift-F10): open, edit, send/remove from devices, convert, remove a format, write metadata, show in Finder, merge, move to Trash
- [x] Native Magnitide library (schema v1) and calibre import, in the app with progress (also `cargo run --release -p magnitide-core --example import_calibre -- <calibre> <dest>`)
- [x] Library safety: single-writer lock (`.magnitide.lock`), warning for libraries on network shares
- [x] Drag and drop (and "Add Books…") to add books: EPUB metadata and covers, file-name fallback, formats grouped, duplicates skipped
- [x] MOBI/AZW/AZW3 metadata and covers (adding books, device matching); "Surname, Given" authors turned around
- [ ] PDF metadata reader (drag and drop uses file names for PDFs today)
- [x] Native EPUB → AZW3 converter (KF8, a port of calibre's writer): Kindles no longer need calibre installed; calibre is a fallback for unusual books
- [ ] MOBI/AZW3 → EPUB (upgrading old Kindle files)
- [x] Adding a new format of a book already in the library (same title and first author) adds it to that book instead of creating another
- [x] Metadata editing (⌘E): all built-in fields, rich-text description, cover change/remove; folder and file names follow title and first author (all-or-nothing with the database)
- [x] Autocomplete for authors, tags, series and publishers in the edit form
- [x] Author sort names: "Sorted as …" under Authors in the edit form, with per-author overrides ("Le Guin, Ursula K.") that apply to all of that author's books
- [x] Custom fields: make, rename and delete them (Settings › Library), see them in the details panel and as list columns, edit them with a book or several at once, sort and search by them (`#pages:300`). calibre libraries’ custom columns show too, read-only
- [x] File name clean-up: names follow edits, and Settings → Library → "Tidy file names" fixes drifted books and reports files missing from disk
- [x] Write metadata and covers back into EPUB/KEPUB files (details panel "Write to file", Settings → Library for every book, optional write on save); only files that differ are rewritten
- [x] Metadata lookup from Open Library and Google Books (your own API key) with a field-by-field matching dialog: "Find Metadata…" in the edit window, sources in Settings → Metadata Sources. On request only; ranking is plain string matching
- [x] E-reader detection (Kobo, Kindle, any calibre-managed USB device): plugged-in devices appear in the top bar, books on them get an "On Device" column, a cover badge and a line in the details panel, and the list can be filtered to books on or not on a device. Matched by calibre's uuid, ISBN, identical file, then title and author; Eject from the device menu
- [x] File transfer: send selected books to a device (the format it reads best; EPUBs get the library's metadata, cover and id written into the copy), remove them (with KOReader's notes), and add a device's other files to the library
- [x] Format conversion: "Convert…" in the details panel and selection bar adds the new format to the book. EPUB → KEPUB (Kobo spans, like kepubify) and EPUB → AZW3 are built in; MOBI, PDF and others use calibre's `ebook-convert` when installed (detected, never bundled). Sending converts on the way when a device's default format is missing, or when a Kindle gets an EPUB-only book
- [x] Send asks which format when a book has several the device reads; Settings → Devices sets a default per kind of device
- [ ] Kindle covers: write cover thumbnails on send and restore the ones the Kindle deletes once online (calibre's workaround)
- [x] Multi-select (⌘-click, Shift-click, ⌘A) with a selection bar; "Merge…" for two records of the same book
- [x] Delete books (⌘⌫, or Delete): their folders move to the Trash after a confirmation
- [x] Bulk editing (⌘E with several books selected): fields show what the books share or "Mixed", only changed fields are saved; add/remove tags, series numbering in list order; "Edit One by One" steps through the selection with Previous/Next
- [x] Omnibus editions and several series per book: an "Omnibus" tick turns the number into a range ("Books 1–3"), "+ Another series" adds more, and "Collects" links an omnibus to the books it contains (shown both ways in the details panel). Magnitide-only; files and e-readers get the first series and number
- [x] Find Metadata for a selection: the Find Metadata dialog steps through the books ("2 / 4", Previous / Skip / Use & Next), saving each one's chosen fields; the next book's search runs ahead
- [x] Shelves: a sidebar of your own shelves (drag covers or rows onto one, or "Add to Shelf" in the right-click menu); click one to show its books (`shelf:"Name"` in search). Clickable series in the details panel
- [x] Shelves on a Kobo: written as collections for Kobo's own app and KOReader when sending (or "Sync Shelves" in the device menu). Only Magnitide's shelves and books are touched; Kobo's database is backed up first
- [ ] MTP devices (newer Kindles)
- [x] Interface size: Settings → Appearance (80–200% in steps; a new size asks to be kept and reverts by itself after 15 s, like a display resolution), or ⌘+ / ⌘− / ⌘0 (View menu on macOS). The whole window zooms, remembered per device; the macOS title bar stays native size, and the window's minimum size grows with it so the toolbar always fits
- [x] Keyboard shortcuts shown the platform's way (⌘E on macOS, Ctrl+E elsewhere), all listed in Settings → Keyboard Shortcuts
- [x] calibre plugins, step 1: Settings → Plugins installs calibre plugin zips, run in your installed calibre with their own settings; metadata-source plugins are asked in Find Metadata alongside the built-in sources (survey and plan: [docs/calibre-plugins.md](docs/calibre-plugins.md))
- [ ] calibre plugins, step 2: GUI plugins (EmbedComicMetadata, EpubSplit, EpubMerge, Count Pages, Comments Cleaner) on a calibre-format shadow library. In progress: GUI plugins run from the toolbar's Plugins button and the book menu, their changes come back into the library, and calibre's Edit Metadata, Remove Books and Convert Books are Magnitide's; the rest of calibre's actions aren't mapped yet
- [ ] calibre plugins, step 3: Ebook Translator, FanFicFare, Goodreads Sync
- [ ] Find Duplicates, built in
- [ ] Built-in reader
- [ ] Export to a calibre library
- [x] Library server (Settings → Library Server, off by default): the open library on your Wi-Fi, read-only, with an optional password. Kobo's browser (and any other) gets a plain, script-free page to browse, search and download from, with KEPUBs made on the fly from EPUBs; KOReader and other reading apps add `http://<address>/opds` as an OPDS catalogue. Shows the address and a QR code
- [x] KOReader progress sync: the library server is also a KOReader sync server (Tools → Progress sync → Custom sync server). Positions are kept per user name in their own database, outside the library
- [ ] Reading positions in the app: match synced positions to library books (by KOReader's fingerprint of the copy on the device) and show how far you are
- [ ] Server mode: a headless server (Docker, NAS) with accounts, Kobo sync
