# magnitide

An AI-free ebook library manager that works with existing calibre libraries.

How the pieces fit, and the decisions behind them: [docs/architecture.md](docs/architecture.md).

## Crates

| Crate | What it is |
|---|---|
| [`app`](crates/app) | The desktop app: Tauri 2, with a Solid + TypeScript UI in [`crates/app/ui`](crates/app/ui). Currently a read-only cover grid with search and a details panel |
| [`core`](crates/core) | `magnitide-core`: the native Magnitide library (schema, migrations, edits), calibre import, adding book files with EPUB metadata. No Tauri code, so the planned server can share it |
| [`catalog`](crates/catalog) | `magnitide-catalog`: the in-memory book model, search and sort that every kind of library loads into |
| [`calibre-compat`](crates/calibre-compat) | Read-only access to calibre libraries (`metadata.db` + book folders), plus a CLI to inspect them |

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

Keys: `/` or ⌘F search · ⌘, settings · ⌘1 covers, ⌘2 list · arrows move · Enter opens details · ⌘E edits · double-click or ⌘O opens the book in its default app · Esc closes.
In the details panel, click a format to open that specific file.
Search accepts `author:`, `tag:`, `series:`, `title:`, `#custom:`, quotes and `-` to exclude.

## Development

```bash
cargo test                             # all crates; also regenerates the UI's TypeScript bindings
pnpm --dir crates/app/ui lint                 # typecheck + formatting (`format` to fix)
cargo run -p calibre-compat -- info    # CLI against the default library
```

Styling is [Tailwind v4](https://tailwindcss.com) utilities in the components. Colours
and shadows come only from the theme (`crates/app/ui/src/themes.css`): each theme
is a block of CSS variables, and `theme.ts` switches between them at runtime by
setting `data-theme` on `<html>`. Built in: light, dark, sepia, and "system"
(follows the OS). To add a theme, copy a block in `themes.css` and register it in
`THEMES` in `theme.ts`.

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
- [ ] (Jamie) Tweak zebra stripe colours: inconsistent across themes. The stripe is `bg-surface-2/40` in `crates/app/ui/src/components/BookTable.tsx`; a per-theme `--row-stripe` variable in `themes.css` would let each theme set its own
- [x] Native Magnitide library (schema v1) and calibre import, in the app with progress (also `cargo run --release -p magnitide-core --example import_calibre -- <calibre> <dest>`)
- [x] Library safety: single-writer lock (`.magnitide.lock`), warning for libraries on network shares
- [x] Drag and drop (and "Add Books…") to add books: EPUB metadata and covers, file-name fallback, formats grouped, duplicates skipped
- [ ] Metadata readers for MOBI/AZW3 and PDF (drag and drop uses file names for these today)
- [ ] Adding a new format to an existing book (today a new format of a book already in the library becomes a separate book)
- [x] Metadata editing (⌘E): all built-in fields, rich-text description, cover change/remove; folder and file names follow title and first author (all-or-nothing with the database)
- [x] Autocomplete for authors, tags, series and publishers in the edit form
- [ ] Editing custom fields
- [x] File name clean-up: names follow edits, and Settings → Library → "Tidy file names" fixes drifted books and reports files missing from disk
- [ ] Write edited metadata back into book files (EPUB OPF), so other apps and devices see it
- [ ] Accessibility scaling: ⌘+ / ⌘− / ⌘0 zoom the whole UI (remembered), respecting the OS text size where available
- [ ] Plugin host for DeDRM (embedded Python)
- [ ] Built-in reader
- [ ] Export to a calibre library
- [ ] Server mode: web UI and OPDS, then accounts, KOReader sync, Kobo sync
