# Magnitide plugins

Magnitide's own plugin API: a plugin is a WebAssembly component that
implements one of the worlds in the `magnitide:plugin` WIT package, packed
with a manifest into a `.mgp` file. It runs sandboxed inside the app, in any
language with a WIT toolchain, and does only what its manifest declares.

This is the forward-looking track beside the calibre plugin host
([calibre-plugins.md](calibre-plugins.md)): calibre plugins run unchanged in
a calibre or in Magnitide's runtime; Magnitide plugins are written against
Magnitide directly, are a small fraction of the size, start in
milliseconds, and work the same on the desktop and on a server. Porting a
calibre metadata source is mostly deleting code: the lookup, the record and
HTTP are the API.

## What a plugin can be

| Kind              | World                    | Exports                                   | Where it shows up                                                       |
| ----------------- | ------------------------ | ----------------------------------------- | ----------------------------------------------------------------------- |
| `metadata-source` | `metadata-source-plugin` | `info()`, `identify(lookup)`              | Find Metadata, beside the built-in sources; Settings › Metadata Sources |
| `import-hook`     | `import-hook-plugin`     | `file-types()`, `on-import(file, format)` | Adding books: runs on each file of its types before it's stored         |
| `file-metadata`   | `file-metadata-plugin`   | `reads()`, `read()`, `writes()`, `write()` | Reading a file's metadata when it's added or re-read; writing metadata back into the file |
| `library-action`  | `library-action-plugin`  | `menu()`, `run(item, selection)`          | The book menu, under Plugins, and the toolbar's Plugins button: runs on the selected books |

These mirror calibre's `Source`, `FileTypePlugin` (on import),
`MetadataReaderPlugin`/`MetadataWriterPlugin` and `InterfaceAction`. In
the app the first three are the same `core::hooks` traits and the same
Find Metadata fan-out the calibre host plugs into: Magnitide's own plugins
are asked first, calibre plugins see what they made of a file. A library
action works on the open library directly, through host functions, where
a calibre GUI plugin works on a calibre-format shadow of it. Each kind is
a world; a plugin implements one.

## The API

The interface definitions are in
[`crates/plugin-sdk/wit/plugin.wit`](../crates/plugin-sdk/wit/plugin.wit),
package `magnitide:plugin@0.1.0`. A plugin names the API version it was built
for in its manifest (`api = "0.1"`); the app refuses other versions with a
clear message. Within a minor version the API only grows.

Imports every world gets:

- `log.log(level, text)`: lines for the plugin's log, shown in Settings ›
  Plugins. The last 400 lines per plugin are kept.
- `http.fetch(request) -> result<response, string>`: HTTP through
  Magnitide, with its proxy settings and user agent. Needs the `network`
  capability, or every call fails with a message saying so. Responses are
  capped at 20 MB and 30 s; the body comes back whole.

`metadata-source` exports:

- `info() -> source-info`: name, description and version, as shown to the
  user. The manifest's `name` is what Magnitide calls the plugin; `info` is
  what the component says about itself (the app checks they agree at
  install).
- `identify(lookup) -> result<list<book-record>, string>`: the lookup is
  what Find Metadata knows (title, authors, an ISBN when there is one).
  Return records best first; an empty list when nothing matched; an error
  only when the source couldn't be asked. The records join the built-in
  sources' and are ranked together, so a plugin needn't rank against them.

`book-record` has the shape Magnitide's own sources produce: `source-key`
(the source's id for the book), `link`, `title`, `subtitle`, `authors`,
`publisher`, `published` (ISO date or a year), `languages` (ISO 639-2),
`identifiers` (`isbn` as an ISBN-13, or the source's own kind such as
`openlibrary`), `tags`, `description-html` (sanitized by Magnitide),
`cover-url` and `pages`. Everything but the title is optional.

`import-hook` exports:

- `file-types() -> list<string>`: lower-case extensions the hook runs on
  (`"cbz"`), or `"*"` for every file. Extensions Magnitide doesn't know
  become addable as books. Asked once, when the plugin is installed.
- `on-import(file, format) -> result<outcome, string>`: run on a file
  before its metadata is read; `file` is its path in the sandbox
  (`/in/book.cbz`), `format` the upper-case format. Return `unchanged`, or
  `replaced("/out/…")` naming a file the hook wrote under `/out/`; its
  extension decides the format it's added as. An error means the file is
  added as it is and the reason shown. Hooks run in turn, each on what the
  one before made of the file; calibre's file-type plugins come after.

`file-metadata` exports:

- `reads() -> list<reads-format>`: upper-case formats the plugin reads,
  each with a `precedence`: `fallback` (used only for formats Magnitide
  can't read itself) or `override` (used first, Magnitide's reader on
  failure). `writes() -> list<string>`: the formats it writes. Both asked
  once, at install.
- `read(file, format) -> result<book-metadata, string>`: the file is at
  `/in/<name>`, read-only.
- `write(file, format, meta) -> result<_, string>`: the file is a writable
  copy at `/out/<name>`; write into it in place. Magnitide moves it over
  the original only when every writer succeeded, so a failing plugin
  leaves the book as it was.

`book-metadata` is Magnitide's `BookMetadata`: `title`, `authors` (name
and an optional sort form), `series` and `series-index`, `publisher`,
`published` (ISO date or a year), `languages`, `identifiers`,
`description` (HTML or text), `tags`, `cover` (image bytes), `uuids`. All
optional: leave out what the file doesn't say.

`library-action` exports:

- `menu() -> list<menu-item>`: the items (`id`, `text`, `needs-selection`)
  shown under the plugin's name in the book menu and the Plugins button.
  Asked once, at install. One item is shown on its own, several as a
  submenu; an item that needs a selection is greyed out without one.
- `run(item, selection) -> result<string, string>`: run the item on the
  selected books, in list order. Return a line to show when done
  ("Tagged 12 books"); an error is shown as one. The run is a job in
  Magnitide's job bar, with the plugin's status lines as its progress.

A library action imports two more interfaces:

- `library`: the open library. `fields()` (the custom fields), `books(ids)`
  (title, authors, series, publisher, published, rating, languages,
  identifiers, tags, description, formats, cover flag and custom values),
  `all-books()`, `search(query)` in Magnitide's search syntax; then, with
  the `write-library` capability, `edit(id, changes)` (only what's set
  changes; an empty string clears a text, a series with an empty name
  clears the series, rating 0 clears it, a custom value of none clears the
  field), `set-cover(id, image)` and `add-format(id, format, file)` (a
  file the plugin wrote under `/out/`, replacing a format the book has);
  with `read-files`, `open-format(id, format)` puts the book's file in the
  sandbox and returns its `/in/…` path. Edits go through the same paths as
  Magnitide's own editor (names synced, files renamed, write-back), and
  the library view reloads when the action is done.
- `ui`: `status(text)` for a line of progress, `progress(done, total)`
  for how far along.

## The sandbox

Each call to a plugin gets a fresh instance: no state survives between
calls, a crash is a plugin error and not an app crash, and a plugin can't
see another's memory. The instance has:

- no filesystem, no environment, no clock beyond WASI's, no sockets. WASI
  preview 2 is linked for the language runtimes' sake (allocation, random
  numbers, stdio into the log) with nothing preopened, except for a call
  that works on a file: then `/in` holds that one file read-only (a hard
  link into a scratch folder, or a copy across volumes) and `/out` is a
  writable scratch folder, both gone when the call is done (a file the
  plugin handed back lives on until Magnitide has stored it). The plugin
  never sees the library, the file's folder or its siblings;
- 256 MB of memory at most;
- 60 s per call, after which it's stopped ("the plugin took too long and
  was stopped"); an import hook gets five minutes, a library action an
  hour;
- only the host functions its capabilities allow. Today's capability is
  `network`.

What a plugin may do is in its manifest and shown in Settings when it's
added. The capabilities are `network` (`http.fetch`), `write-library`
(`library.edit`, `set-cover`, `add-format`) and `read-files`
(`library.open-format`). A plugin that asks for nothing can only compute
over its input and read the library's metadata.

## Packaging: `.mgp`

A zip with a `manifest.toml` at its root, the component, and anything else
the plugin ships (resources are kept beside the component; a future
`resources` import will read them):

```toml
name = "Open Library (plugin)"       # what Settings and Find Metadata call it
version = "0.1.0"
author = "Magnitide"
description = "Looks books up on Open Library"
api = "0.1"                          # magnitide:plugin API version
kind = "metadata-source"
capabilities = ["network"]           # [] for none
component = "plugin.wasm"            # the component inside the zip
```

Installed plugins live under `<app config>/plugins/<slug>/` (the slug is
the name, lower-cased, non-alphanumerics as dashes: `open-library-plugin`),
with `plugins.json` beside them for the enabled flags. Adding a `.mgp`
whose name is already installed replaces it (an update). Removing deletes
the folder.

## Writing one in Rust

The SDK is [`crates/plugin-sdk`](../crates/plugin-sdk) (`magnitide-plugin-sdk`):
the generated bindings, an export macro and helpers for HTTP and logging.
The example is [`plugins/openlibrary-source`](../plugins/openlibrary-source),
the same lookup as the built-in Open Library source, in ~120 lines:

```rust
use magnitide_plugin_sdk::prelude::*;

struct OpenLibrary;

impl MetadataSource for OpenLibrary {
    fn info() -> SourceInfo {
        SourceInfo { name: "Open Library (plugin)".into(), description: "…".into(), version: "0.1.0".into() }
    }

    fn identify(lookup: Lookup) -> Result<Vec<BookRecord>, String> {
        let url = format!("https://openlibrary.org/search.json?title={}", lookup.title);
        log::info(format!("GET {url}"));
        let body = http::get_with(&url, &[("Accept", "application/json")])?;
        // parse, map to BookRecord…
        Ok(vec![])
    }
}

magnitide_plugin_sdk::export_metadata_source!(OpenLibrary);
```

Start from the template, build and pack:

```bash
rustup target add wasm32-wasip2
scripts/new-plugin.sh plugins/my-source "My Source" metadata-source   # or import-hook, file-metadata, library-action
scripts/build-plugin.sh plugins/my-source                             # → target/plugins/my-source.mgp
scripts/build-plugin.sh plugins/openlibrary-source                    # → target/plugins/openlibrary-source.mgp
```

The template ([`plugins/template`](../plugins/template)) holds a manifest,
a skeleton of each kind under `kinds/`, a README for the plugin's own
repository and a GitHub workflow that builds the `.mgp` on every push and
attaches it to a release on a `v*` tag. Outside this repository, the SDK
is a git dependency on it (see the template's `Cargo.toml`).

To check and run a package without the app, the host crate's `mgp` tool:

```bash
cargo run -p magnitide-plugins-wasm --bin mgp -- check target/plugins/my-source.mgp
cargo run -p magnitide-plugins-wasm --bin mgp -- identify target/plugins/my-source.mgp "Dune" "Frank Herbert"
cargo run -p magnitide-plugins-wasm --bin mgp -- read target/plugins/my-format.mgp book.cbz
cargo run -p magnitide-plugins-wasm --bin mgp -- hook target/plugins/my-hook.mgp book.epub
```

It installs the package into a temporary folder with the capabilities its
manifest asks for, prints the manifest and what the component handles
(`check`), or runs it, with the plugin's log after the answer.

An import hook or a file-metadata plugin implements `ImportHook` or
`FileMetadata` from the same prelude and is exported with
`export_import_hook!` or `export_file_metadata!`. It reads and writes
the file with `std::fs` at the paths it's given (`/in/…`, `/out/…`). A
library action implements `LibraryAction` (`export_library_action!`) and
calls `library::books`, `library::edit`, `ui::status` and the rest from
the prelude. The fixtures in `crates/plugins-wasm/tests/fixtures` are the
smallest examples of each.

The plugin's `Cargo.toml` is its own workspace (`[workspace]`), with
`crate-type = ["cdylib"]` and the SDK as a path or git dependency. Any crate
that compiles for `wasm32-wasip2` without threads or sockets is usable
(`serde_json`, `scraper`, `regex`…). `opt-level = "s"` and `strip = true`
keep the component small (the example is 64 KB zipped).

Other languages: anything that produces a component from the WIT
(`componentize-py`, `jco` for JavaScript, TinyGo's `wit-bindgen-go`) works
the same way; the manifest and `.mgp` are identical.

## Testing

`cargo test -p magnitide-plugins-wasm` builds four fixture plugins for
`wasm32-wasip2` (the target must be installed), one per kind, and runs
them through the host: installing, a lookup, an import hook that rewrites
a file into another format (and checks its sandbox has nothing else in
it), a file-metadata plugin reading and writing, a library action editing
an in-memory library (refused without `write-library`, book files through
the sandbox, a format added), errors, a panic, a runaway loop that the
deadline stops, the network gate, bad packages.
`MAGNITIDE_PLUGIN_NETWORK=1` adds a test that builds the example plugin and
asks Open Library through it.

## In the app

`crates/plugins-wasm` is the host: the engine (wasmtime), the manifest and
registry, capability gating, the host functions, the per-call file sandbox
and the limits; it implements `core::hooks::ImportHooks` and
`MetadataPlugins`, and runs library actions over a `LibraryAccess` the
app provides. The app keeps one `WasmPlugins` beside the calibre
`Plugins`: Find Metadata asks both, `AppHooks` fans adding and metadata
reading/writing out to both (Magnitide's plugins first), the plugin menus
show Magnitide's actions first and calibre's after, and Settings ›
Plugins lists Magnitide plugins first, calibre plugins below, each with
what it handles. The TypeScript side sees `NativePlugin` (`manifest`,
`slug`, `enabled`, `handles`).
