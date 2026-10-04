# magnitide

An AI-free ebook library manager that works with existing calibre libraries.

How the pieces fit, and the decisions behind them: [docs/architecture.md](docs/architecture.md).

## Crates

| Crate                                     | What it is                                                                                                                                                                          |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`app`](crates/app)                       | The desktop app: Tauri 2, with a Solid + TypeScript UI in [`crates/app/ui`](crates/app/ui). Browse, search, edit, import and look up metadata                                       |
| [`core`](crates/core)                     | `magnitide-core`: the native Magnitide library (schema, migrations, edits), calibre import, adding book files with EPUB metadata. No Tauri code, so the planned server can share it |
| [`catalog`](crates/catalog)               | `magnitide-catalog`: the in-memory book model, search and sort that every kind of library loads into                                                                                |
| [`sources`](crates/sources)               | `magnitide-sources`: metadata lookup from Open Library and Google Books, on request only                                                                                            |
| [`calibre-compat`](crates/calibre-compat) | Read-only access to calibre libraries (`metadata.db` + book folders), plus a CLI to inspect them                                                                                    |

## Setup

Needs Rust, Node with [pnpm](https://pnpm.io), and the Tauri CLI:

```bash
cargo install tauri-cli --version "^2" --locked
pnpm --dir crates/app/ui install
```

## Running

```bash
cd crates/app
cargo tauri dev                  # live reload: UI changes appear on save, Rust changes rebuild
cargo tauri build --bundles app  # release Magnitide.app in target/release/bundle/macos/
```

It opens the library calibre last used, falling back to `~/Calibre Library`.
Click the library name in the top-left to open a different one. The library
is only ever read; cover thumbnails are cached in the app's cache folder
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

Styling is [Tailwind v4](https://tailwindcss.com) utilities in the components. Colours
and shadows come only from the theme (`crates/app/ui/src/themes.css`), in two layers:
a palette named after [Catppuccin's](https://github.com/catppuccin/catppuccin/blob/main/docs/style-guide.md)
(crust, mantle, base, surface 0–2, overlay 0–2, subtext 0–1, text, and the accents),
and roles the UI paints with (`--bg`, `--panel`, `--popover`, `--field`, `--element`,
`--fg`, `--link`, `--accent`, `--hover`, `--selection`, `--highlight`, `--row-stripe`,
`--focus`, status colours, `--inverse`, `--scrim`…), derived from the palette following
the Catppuccin and [Dracula](https://draculatheme.com/spec) role tables. A theme is a
palette plus any role overrides; `theme.ts` switches between them at runtime by setting
`data-theme` on `<html>`. Built in: light, dark, sepia, the four Catppuccin flavours,
Dracula and Alucard, and "system" (follows the OS). To add a theme, add a block in
`themes.css` (a Catppuccin port pastes its palette as is) and register it in `THEMES` in
`theme.ts`.

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
- [ ] Add clear search button
- [ ] Table sidebar allow resizing of width by dragging its edge, but let's set a min-width to prevent it from collapsing too much
- [x] Add slight app blur behind dialogs (every dialog dims and blurs the app behind it, tinted per theme)
- [ ] Covers view should keep selected book in view after sidebar opens & grid resize
- [ ] Make "Send to device" & "Convert" actions more discoverable/prominent (they currently look like the format badges)
- [ ] Implement table column reordering
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
- [ ] Editing custom fields
- [x] File name clean-up: names follow edits, and Settings → Library → "Tidy file names" fixes drifted books and reports files missing from disk
- [x] Write metadata and covers back into EPUB/KEPUB files (details panel "Write to file", Settings → Library for every book, optional write on save); only files that differ are rewritten
- [x] Metadata lookup from Open Library and Google Books (your own API key) with a field-by-field matching dialog: "Find Metadata…" in the edit window, sources in Settings → Metadata Sources. On request only; ranking is plain string matching, no AI
- [x] E-reader detection (Kobo, Kindle, any calibre-managed USB device): plugged-in devices appear in the top bar, books on them get an "On Device" column, a cover badge and a line in the details panel, and the list can be filtered to books on or not on a device. Matched by calibre's uuid, ISBN, identical file, then title and author; Eject from the device menu
- [x] File transfer: send selected books to a device (the format it reads best; EPUBs get the library's metadata, cover and id written into the copy), remove them (with KOReader's notes), and add a device's other files to the library
- [x] Format conversion: "Convert…" in the details panel and selection bar adds the new format to the book. EPUB → KEPUB (Kobo spans, like kepubify) and EPUB → AZW3 are built in; MOBI, PDF and others use calibre's `ebook-convert` when installed (detected, never bundled). Sending converts on the way when a device's default format is missing, or when a Kindle gets an EPUB-only book
- [x] Send asks which format when a book has several the device reads; Settings → Devices sets a default per kind of device
- [ ] Kindle covers: write cover thumbnails on send and restore the ones the Kindle deletes once online (calibre's workaround)
- [x] Multi-select (⌘-click, Shift-click, ⌘A) with a selection bar; "Merge…" for two records of the same book
- [x] Delete books (⌘⌫, or Delete): their folders move to the Trash after a confirmation
- [x] Bulk editing (⌘E with several books selected): fields show what the books share or "Mixed", only changed fields are saved; add/remove tags, series numbering in list order; "Edit One by One" steps through the selection with Previous/Next
- [x] Omnibus editions and several series per book: an "Omnibus" tick turns the number into a range ("Books 1–3"), "+ Another series" adds more, and "Collects" links an omnibus to the books it contains (shown both ways in the details panel). Magnitide-only; files and e-readers get the first series and number
- [ ] Bulk metadata lookup for a selection
- [ ] MTP devices (newer Kindles) and Kobo collections/shelves
- [ ] Accessibility scaling: ⌘+ / ⌘− / ⌘0 zoom the whole UI (remembered), respecting the OS text size where available
- [ ] Plugin host for DeDRM (embedded Python)
- [ ] Built-in reader
- [ ] Export to a calibre library
- [ ] Server mode: web UI and OPDS, then accounts, KOReader sync, Kobo sync
