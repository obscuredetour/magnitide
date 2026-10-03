# magnitide

An AI-free ebook library manager that works with existing calibre libraries.

## Crates

| Crate | What it is |
|---|---|
| [`app`](crates/app) | The desktop app (Tauri 2). Currently a read-only cover grid with search and a details panel |
| [`calibre-compat`](crates/calibre-compat) | Read-only access to calibre libraries (`metadata.db` + book folders), plus a CLI to inspect them |

## Running

```bash
cargo run -p magnitide
```

It opens the library calibre last used, falling back to `~/Calibre Library`.
Click the library name in the top-left to open a different one. The library
is only ever read; cover thumbnails are cached in the app's cache folder
(`~/Library/Caches/dev.magnitide.app` on macOS).

Keys: `/` or ⌘F search · arrows move · Enter opens details · Esc closes.
Search accepts `author:`, `tag:`, `series:`, `title:`, `#custom:`, quotes and `-` to exclude.

```bash
cargo test                      # all crates
cargo run -p calibre-compat -- info   # CLI against the default library
```
