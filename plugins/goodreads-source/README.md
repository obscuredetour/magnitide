# Goodreads

The calibre Goodreads metadata source, ported to Magnitide's plugin API.
A title and author search on goodreads.com, the likeliest results' pages,
and each page's embedded book data; an ISBN goes straight to the book's
page. Returns title, authors (the contributors in the Author role),
the series and the book's number in it, publisher, publication date,
ISBN and ASIN, language, up to eight genres as tags, the description and
the cover.

- Build: `scripts/build-plugin.sh plugins/goodreads-source` (from the
  Magnitide repository) → `target/plugins/goodreads-source.mgp`.
- Test the parser: `cargo test` in this folder runs on saved pages in
  `tests/fixtures`; `MAGNITIDE_PLUGIN_NETWORK=1 cargo test -p
  magnitide-plugins-wasm the_goodreads` asks the live site through the host.
- Try it: `cargo run -p magnitide-plugins-wasm --bin mgp -- identify
  target/plugins/goodreads-source.mgp "Project Hail Mary" "Andy Weir"`.

Goodreads has no public API; this reads its website, so it can break when
the site changes. Goodreads also answers a burst of requests with a
browser check for a while; the plugin says so rather than returning
nothing.
