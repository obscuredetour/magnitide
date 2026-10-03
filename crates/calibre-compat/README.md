# calibre-compat

Prototype: read an existing calibre library (`metadata.db` + book folders)
natively in Rust, without calibre installed. This tests the riskiest
assumption of a calibre rewrite: that existing libraries can be used in place.

**Read-only by design.** The database is opened `SQLITE_OPEN_READ_ONLY` +
`PRAGMA query_only`, and a test checks that opening a library leaves the file
byte-identical with no journal/WAL files created. Safe to point at a library
calibre currently has open.

## Usage

From the repository root:

```bash
cargo build --release
B=target/release/calibre-compat

$B info                        # uses calibre's last-used library (from global.py.json)
$B -l ~/"Calibre Library" info
$B -l ~/"Calibre Library" list -s 'author:"le guin" tag:fantasy -#read:yes' --sort series -n 20
$B -l ~/"Calibre Library" show 1234
$B -l ~/"Calibre Library" verify   # every format file + cover referenced by the DB exists?
$B -l ~/"Calibre Library" export --pretty > library.json
$B -l ~/"Calibre Library" bench

$B synth /tmp/fake-lib --books 50000 [--with-files]   # synthetic library for benchmarks
```

`-l` accepts the library folder or the `metadata.db` path. `CALIBRE_LIBRARY`
also works.

## What it reads

| Data | Status |
|---|---|
| Title, sort, authors (in order), author sort, series + index, tags, publisher, rating, languages (in order), identifiers, comments, dates, uuid, path, cover flag | ✅ |
| Formats and on-disk file paths (`<path>/<name>.<fmt>`, `cover.jpg`) | ✅ |
| Custom columns: text, multi-text, series (+index), enumeration, rating, int, float, bool, datetime, comments | ✅ |
| Composite custom columns | Template is read; **not evaluated** (needs calibre's template language) |
| `preferences` table (JSON) | ✅ via `Library::preference(key)` |
| Library UUID, schema version, application id | ✅ |
| Annotations, last-read positions, notes, full-text-search DB, conversion options | ❌ not yet |
| Search | A small subset: `field:value`, `-negation`, quotes, AND. No OR/parens/regex/relational ops |

Dates use calibre's rules (UTC; year ≤ 101 means "undefined").

## Verification

- `tests/calibre_fixture.rs` loads calibre's own test library
  (`src/calibre/db/tests/metadata.db`, schema v28, one custom column of each
  type) and asserts the field values from calibre's
  `db/tests/reading.py::test_read`. All pass.
- Unit tests cover date parsing, the search tokenizer, and `title_sort`.
- Synthetic 50,000-book library (60 MB `metadata.db`, 7 custom columns),
  Apple M3 Max, release build: **open + full load ≈ 185 ms**, unindexed
  search across all books ≈ 35 ms.
- **Not yet run against a real, years-old personal library.** That is the
  next test to do (read-only, so it's safe).

## Compatibility traps found (matter for the write path)

1. **calibre's triggers call app-defined SQL functions.** A plain SQLite
   client can't insert a book, or even `UPDATE books SET has_cover = 1`:
   `no such function: title_sort`. A writer must register `title_sort` and
   `uuid4` (done in `sqlfuncs.rs`, with a test). Views additionally need
   `books_list_filter`, `concat`/`sortconcat` (aggregates), and others.
2. **`title_sort` is language-aware in calibre** (per-language article
   lists). Only the English default is ported. A mismatch would produce
   sort values that differ from calibre's.
3. **Composite columns need calibre's template language** to display
   anything. Common in real libraries.
4. **Schema versions:** tested against `user_version` 28. Older libraries
   still read; newer ones get a warning. Writing must refuse unknown versions.
5. **Custom columns are dynamic tables** (`custom_column_N`,
   `books_custom_column_N_link`) whose layout depends on `datatype` and
   `normalized`. Ported in `library.rs` (read) and `synth.rs` (create).

## Layout

```
src/library.rs   open + bulk load (one query per table, like calibre's cache)
src/model.rs     Book, Format, CustomColumn, CustomValue
src/search.rs    tiny search-syntax subset
src/sqlfuncs.rs  calibre's SQL functions needed by triggers; title_sort port
src/synth.rs     synthetic-library generator (calibre schema + custom-column DDL)
src/locate.rs    find calibre's current library from its config
schema/          calibre's metadata_sqlite.sql (v28)
tests/fixtures/  calibre's test metadata.db
```

License: GPL-3.0-or-later. The schema, test fixture and ported logic come
from calibre (GPLv3).
