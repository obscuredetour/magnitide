# magnitide

An AI-free ebook library manager that works with existing calibre libraries.

How the pieces fit, and the decisions behind them: [docs/architecture.md](docs/architecture.md).

## Crates

| Crate | What it is |
|---|---|
| [`app`](crates/app) | The desktop app: Tauri 2, with a Solid + TypeScript UI in [`crates/app/ui`](crates/app/ui). Currently a read-only cover grid with search and a details panel |
| [`core`](crates/core) | `magnitide-core`: the native Magnitide library (schema, migrations, edits; import next). No Tauri code, so the planned server can share it |
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

Keys: `/` or ⌘F search · ⌘, settings · ⌘1 covers, ⌘2 list · arrows move · Enter opens details · double-click or ⌘O opens the book in its default app · Esc closes.
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
- [ ] Native Magnitide library: schema v1 and calibre import done in `core` (`cargo run --release -p magnitide-core --example import_calibre -- <calibre> <dest>`); next: open native libraries and import from the app
- [ ] Drag and drop to add books
- [ ] Metadata editing
- [ ] Accessibility scaling: ⌘+ / ⌘− / ⌘0 zoom the whole UI (remembered), respecting the OS text size where available
- [ ] Built-in reader
- [ ] Export to a calibre library
- [ ] Server mode: web UI and OPDS, then accounts, KOReader sync, Kobo sync
