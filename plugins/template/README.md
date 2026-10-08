# My Plugin

A Magnitide plugin, from the template. Magnitide plugins are WebAssembly
components on the `magnitide:plugin` API, packed with a manifest into a
`.mgp` file; the API is described in the Magnitide repository's
`docs/plugin-api.md`.

## Layout

- `manifest.toml`: name, version, kind and capabilities. What Settings
  shows, and what the plugin may do.
- `src/lib.rs`: the plugin. The template starts as a metadata source;
  the other kinds are in `kinds/`: copy the one you want over
  `src/lib.rs` and set `kind` in the manifest to match.
- `resources/` (optional): files packed beside the component.

| Kind              | Implements       | Export macro               |
| ----------------- | ---------------- | -------------------------- |
| `metadata-source` | `MetadataSource` | `export_metadata_source!`  |
| `import-hook`     | `ImportHook`     | `export_import_hook!`      |
| `file-metadata`   | `FileMetadata`   | `export_file_metadata!`    |
| `library-action`  | `LibraryAction`  | `export_library_action!`   |

## Building

```bash
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
```

Inside the Magnitide repository, `scripts/build-plugin.sh <this folder>`
builds and packs the `.mgp` into `target/plugins/`. Elsewhere, zip
`manifest.toml`, the component as `plugin.wasm` and any `resources/`
folder at the root of a zip named `<plugin>.mgp`; the workflow in
`.github/workflows/release.yml` does this on every push and attaches the
file to a release when a `v*` tag is pushed.

## Trying it

Without the app, Magnitide's `mgp` tool checks a package and runs it:

```bash
cargo run -p magnitide-plugins-wasm --bin mgp -- check target/plugins/my-plugin.mgp
cargo run -p magnitide-plugins-wasm --bin mgp -- identify target/plugins/my-plugin.mgp "Dune" "Frank Herbert"
cargo run -p magnitide-plugins-wasm --bin mgp -- read target/plugins/my-plugin.mgp book.cbz
cargo run -p magnitide-plugins-wasm --bin mgp -- hook target/plugins/my-plugin.mgp book.epub
```

In the app: Settings › Plugins › Add Plugin…, choose the `.mgp`. Adding
it again replaces the installed copy. The plugin's log is behind its
Log button there.

## What a plugin can rely on

- No filesystem, environment or network, except what the manifest asks
  for and what the call hands in (`/in`, `/out`).
- 256 MB of memory; a minute per call (five for an import hook, an hour
  for a library action).
- A fresh instance per call: keep nothing in statics between calls.
- Any crate that compiles for `wasm32-wasip2` without threads or sockets.
