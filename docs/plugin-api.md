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

| Kind              | World                    | Exports                   | Where it shows up                              |
| ----------------- | ------------------------ | ------------------------- | ---------------------------------------------- |
| `metadata-source` | `metadata-source-plugin` | `info()`, `identify(lookup)` | Find Metadata, beside the built-in sources; Settings › Metadata Sources |

Planned, in the order the calibre host grew: `import-hooks` (file types to
accept, and a hook on each file added), `metadata-plugins` (readers and
writers for formats), `library-action` (menu items that run on the selected
books, with host functions for reading and editing the library, progress and
questions). Each is a new world; a plugin implements one.

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

## The sandbox

Each call to a plugin gets a fresh instance: no state survives between
calls, a crash is a plugin error and not an app crash, and a plugin can't
see another's memory. The instance has:

- no filesystem, no environment, no clock beyond WASI's, no sockets. WASI
  preview 2 is linked for the language runtimes' sake (allocation, random
  numbers, stdio into the log) with nothing preopened;
- 256 MB of memory at most;
- 60 s per call, after which it's stopped ("the plugin took too long and
  was stopped");
- only the host functions its capabilities allow. Today's capability is
  `network`.

What a plugin may do is in its manifest and shown in Settings when it's
added. A plugin that asks for nothing can only compute over its input.

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

Build and pack:

```bash
rustup target add wasm32-wasip2
scripts/build-plugin.sh plugins/openlibrary-source   # → target/plugins/openlibrary-source.mgp
```

The plugin's `Cargo.toml` is its own workspace (`[workspace]`), with
`crate-type = ["cdylib"]` and the SDK as a path or git dependency. Any crate
that compiles for `wasm32-wasip2` without threads or sockets is usable
(`serde_json`, `scraper`, `regex`…). `opt-level = "s"` and `strip = true`
keep the component small (the example is 64 KB zipped).

Other languages: anything that produces a component from the WIT
(`componentize-py`, `jco` for JavaScript, TinyGo's `wit-bindgen-go`) works
the same way; the manifest and `.mgp` are identical.

## Testing

`cargo test -p magnitide-plugins-wasm` builds a fixture plugin for
`wasm32-wasip2` (the target must be installed) and runs it through the
host: installing, a lookup, an error, a panic, a runaway loop that the
deadline stops, the network gate, bad packages.
`MAGNITIDE_PLUGIN_NETWORK=1` adds a test that builds the example plugin and
asks Open Library through it.

## In the app

`crates/plugins-wasm` is the host: the engine (wasmtime), the manifest and
registry, capability gating, the host functions and the sandbox limits. The
app keeps one `WasmPlugins` beside the calibre `Plugins`; Find Metadata asks
both, and Settings › Plugins lists Magnitide plugins first, calibre plugins
below. The TypeScript side sees `NativePlugin` (`manifest`, `slug`,
`enabled`).
