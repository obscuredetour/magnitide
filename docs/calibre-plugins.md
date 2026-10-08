# calibre plugin compatibility

Goal: let people keep using the calibre plugins they rely on, unchanged, in
Magnitide, so plugin authors don't have to port anything and users don't lose
features by switching. This page is the survey that sizes it and the design it
points to.

## Survey (2026-10-06)

Ten widely used GUI plugins, read with a static analyser (Python `ast`): every
calibre and Qt import, every call on the library API, every use of calibre's
main window, background jobs, and calls into other plugins.

| Plugin             |  Lines | Library methods | Writes | Custom columns | Main-window hooks | Jobs | Calls other plugins                                                               |
| ------------------ | -----: | --------------: | ------ | -------------- | ----------------: | ---- | --------------------------------------------------------------------------------- |
| EmbedComicMetadata |  2,013 |               9 | yes    | yes            |                 4 | no   | –                                                                                 |
| Count Pages        |  7,824 |             10+ | yes    | yes            |                13 | yes  | –                                                                                 |
| Comments Cleaner   |  1,803 |              1+ | yes    | no             |                 0 | no   | Edit Metadata                                                                     |
| Ebook Translator   | 13,269 |               7 | yes    | no             |                 9 | yes  | –                                                                                 |
| FanFicFare         |  8,916 |              31 | yes    | yes            |                21 | yes  | Reading List, Generate Cover, EpubMerge, Convert Books, Remove Books, Count Pages |
| Find Duplicates    |  5,179 |              28 | yes    | yes            |                21 | no   | –                                                                                 |
| Goodreads Sync     | 10,591 |              27 | yes    | yes            |                12 | no   | –                                                                                 |
| EpubMerge          |  3,713 |              21 | yes    | yes            |                15 | yes  | Edit Metadata, Remove Books                                                       |
| EpubSplit          |  3,707 |              15 | yes    | yes            |                12 | no   | Choose Library, Edit Metadata                                                     |
| Reading List       |  4,167 |              16 | yes    | yes            |                23 | no   | –                                                                                 |

(“+”: the analyser undercounts calls made through a variable holding
`new_api`, e.g. Count Pages' `db_ref.set_field`.)

What they need, taken together (61,000 lines of plugin code):

- **All ten are `InterfaceAction` plugins**: Qt menus, dialogs and settings
  screens (`qt.core`, i.e. PyQt6 in current calibre).
- **67 calibre modules**, 17 of them from `calibre.gui2`. Used by five or more:
  `calibre.customize`, `calibre.gui2`, `calibre.gui2.actions`,
  `calibre.utils.config` (`JSONConfig`), `calibre.utils.date`,
  `calibre.constants`, `calibre.ebooks.metadata`,
  `calibre.gui2.dialogs.confirm_delete`, `calibre.gui2.keyboard`,
  `calibre.ptempfile`, `calibre.utils.localization`, `calibre.gui2.preferences`.
  Several use calibre's ebook machinery: the conversion pipeline (Ebook
  Translator), `oeb.polish.container` (EpubMerge, EpubSplit), `oeb.iterator`
  and word counting (Count Pages, FanFicFare).
- **The library API, mostly the legacy one**: 70 distinct methods on calibre's
  old `LibraryDatabase` (`get_metadata`, `get_custom`/`set_custom`,
  `has_format`, `format_abspath`, `create_book_entry`, `add_format_with_hooks`,
  `prefs`, `field_metadata`, `commit`, …) and more on `new_api` (`set_field`,
  `get_proxy_metadata`, …). **Custom columns** are central: eight of the ten
  read or write them, and several ask the user to create columns for them.
- **calibre's main window** (`self.gui`): 31 attributes, chiefly the book list
  (`library_view`: selection, `model().refresh_ids`, `current_changed`,
  `books_added`, `custom_columns`), `status_bar`, `keyboard` (shortcuts),
  `job_manager`, `tags_view`, `search`, and `iactions` for other actions
  (Edit Metadata, Remove Books, Convert Books, and other plugins).
- **Background jobs** in four (calibre's `job_manager.run_job`, `ParallelJob`,
  `fork_job`).

Simpler plugin types are much easier: kiwidude's metadata sources (Goodreads,
Barnes & Noble, FictionDB, Fantastic Fiction, Baen; 470–1,400 lines each)
subclass `calibre.ebooks.metadata.sources.base.Source` and use only metadata
and utility modules, no GUI or library.

## Design

Plugins run in one of three hosts, chosen in Settings › Plugins, behind the
same protocol, shadow library, window stand-in and hooks:

- **An installed calibre** (the default when one is found): its
  `calibre-debug` runs the host package (`crates/plugins/python/host`) on
  calibre's own code, with plugin settings kept in Magnitide's folder, never
  calibre's. Plugins get the real calibre API; what's Magnitide's is only
  the library and window stand-in. calibre 6 or later.
- **calibre downloaded by Magnitide**: calibre's own release for this
  computer (`scripts/calibre-release/calibre.json` pins the version, the
  file per target and the SHA-512 calibre publishes at
  calibre-ebook.com/signatures/), fetched from calibre's GitHub release
  with download.calibre-ebook.com as the fallback, unpacked under the app's
  data folder (tar.xz on Linux; the disk image through hdiutil on macOS;
  the MSI through msiexec's administrative install on Windows, which needs
  no elevation) and run exactly like an installed calibre. About 180 to
  350 MB. `scripts/calibre-release/pin.py <version>` moves to a newer
  calibre; `--rust-table` prints the table for `calibre.rs`.
- **Magnitide's own plugin runtime** (for people who want nothing of
  calibre): the same host package on a downloaded CPython + PyQt6, with a
  `calibre` package written for Magnitide (`crates/plugins/python/compat`)
  that mirrors calibre's API closely enough that plugins written for
  calibre 5 to 9 load and run.

"Whichever is there" (the default) takes them in that order. The host
tests and the plugin corpus run under the calibre hosts and the runtime
(`MAGNITIDE_PLUGIN_HOST=calibre|runtime`), which makes calibre the oracle
for the compat package: a plugin that works there and fails on ours points
at a gap. `MAGNITIDE_TEST_CALIBRE_DOWNLOAD=1` runs a test that downloads
calibre and hosts a plugin in it.

1. **Runtime: Magnitide's own.** A CPython 3.14 from python-build-standalone
   with PyQt6 (Core, Gui, Widgets, Network, PrintSupport, Svg, OpenGL, DBus),
   lxml, Pillow, regex, html5lib, css-parser, BeautifulSoup, mechanize,
   dateutil, charset-normalizer, pypdf, html2text, markdown, msgpack, apsw and
   xxhash, stripped to ~170 MB on disk (~60 MB downloaded). Built per
   platform by `.github/workflows/plugin-runtime.yml`
   (`scripts/plugin-runtime/`), attached to the GitHub release
   `plugin-runtime-v<N>`, and downloaded by Magnitide on request (Settings ›
   Plugins); the archive's sha256 is compiled into the app (`RELEASES` in
   `crates/plugins/src/runtime.rs`), the download resumes, and the install
   is atomic (`<app local data>/plugin-runtime/<N>/`).
2. **The `calibre` package: Magnitide's.** `crates/plugins/python/` is
   embedded in the binary (`include_dir!`) and extracted beside the app's
   settings. It mirrors calibre 9.15's public API (`numeric_version` is
   `(9, 15, 0)`; `calibre.constants.MAGNITIDE` is `True`): the plugin loader
   (`calibre.customize.zipplugin`, with `calibre_plugins.<name>` imports,
   `get_resources`, `get_icons`, `load_translations`), the plugin base
   classes and registry (`calibre.customize`, `customize.ui`), `Metadata`,
   `calibre.utils.config` (`JSONConfig`, `prefs`, `tweaks`), dates, ICU-style
   collation in pure Python, logging, `calibre.ebooks.metadata` (title and
   author sorting, ISBNs, OPF, EPUB/MOBI/PDF/TXT/FB2 readers and writers),
   metadata sources, `calibre.db` (`Cache`/`LibraryDatabase` over stdlib
   sqlite3 with calibre's SQL functions and search grammar), the template
   language (a subset; `program:` isn't), jobs and IPC workers, the GUI
   classes plugins subclass (`InterfaceAction`, dialogs, preferences,
   widgets, delegates, the comments editor), EPUB editing
   (`oeb.polish.container`), the spine iterator, images and generated
   covers. Modules Magnitide can't provide (the conversion pipeline, device
   drivers, the content server, the viewer and editor, PoDoFo, unrar) exist
   as stubs that raise an `ImportError` naming what's missing, and a static
   scan at install time lists them in Settings (`magnitide_host/coverage.py`).
   Every module's docstring names what it mirrors and its gaps. Old plugins'
   `PyQt5` imports are redirected to PyQt6, with PyQt5-style unscoped enum
   names added.
3. **Library: a calibre-format shadow library.** Magnitide keeps a
   calibre-format copy of the open library's metadata (`metadata.db`, same
   ids, with the book files linked, not copied) in its cache, written by
   `calibre_compat::write`. Plugins use the compat `calibre.db` on it; when
   an action finishes (or hands over to Magnitide mid-run), Magnitide reads
   back what changed (fields, custom columns, identifiers, covers, formats,
   new or removed books) and applies it through its normal edit path, so its
   own rules (renaming, write-back, undo-safety) still hold. The shadow is
   never one of the user's calibre libraries.
4. **Window: a stand-in for calibre's main window** (`magnitide_host/gui`):
   `library_view` (the selection is Magnitide's), `current_db`, `status_bar`
   (Magnitide's toasts), the job manager (jobs show in Magnitide's job bar),
   the keyboard manager, `search` (Magnitide's search box: read, and set
   through Magnitide), and `iactions` for calibre's own actions mapped to
   Magnitide's: Edit Metadata (the editor), Remove Books (asks, then
   deletes), metadata download (Find Metadata), Convert Books (a format
   picker and Magnitide's conversion), Add Books (into the shadow), View
   (opens the book), Choose Library (inert). The plugin waits at these as it
   would in calibre; Magnitide applies what it did so far, does its part,
   sets the result in the shadow and lets it go on. Plugin menus appear in
   the book menu under Plugins; plugin dialogs open as their own Qt windows.
5. **The other kinds on the same host.** Metadata sources join Find Metadata.
   File-type plugins run when books are added (`on_import` rewrites the
   file before Magnitide reads it; `postimport`/`postadd` run on the shadow
   afterwards), after conversions (`postconvert`) and when a format is
   removed (`postdelete`), through the `ImportHooks` trait in `core::hooks`.
   Metadata readers and writers plug into `core::metadata::read_with` and
   write-back through `MetadataPlugins` (a reader with priority above
   calibre's own beats Magnitide's; otherwise it's a fallback for formats
   Magnitide can't read). Preferences plugins and plugins with their own
   `config_widget` get **Configure…** in Settings. Library-closed plugins
   run when the library is switched or the app quits.

### What each kind of plugin gets

| Kind                     | Runs | Notes                                                                                      |
| ------------------------ | ---- | ------------------------------------------------------------------------------------------ |
| Metadata source          | yes  | Find Metadata asks it; its options dialog works.                                           |
| Interface action (GUI)   | yes  | On the shadow library; see 4 above.                                                        |
| File type                | yes  | `on_import`, `postimport`, `postadd`, `postconvert`, `postdelete`; not `preprocess`/`postprocess` (conversion). |
| Metadata reader / writer | yes  | For the formats they declare; priority decides against Magnitide's own readers.            |
| Preferences              | yes  | Its page opens from Settings › Plugins › Configure….                                      |
| Library closed           | yes  | On switching library and on quit.                                                          |
| Conversion input/output  | no   | calibre's conversion pipeline isn't run; Magnitide converts natively or with `ebook-convert`. |
| Catalog                  | no   | No catalog generator yet.                                                                  |
| Device, store, viewer, editor tool | no | Magnitide has its own device support; no store, viewer or editor.                  |

### Checked against real plugins

`tests/plugins/manifest.json` lists plugins (kiwidude's Count Pages,
Reading List, Goodreads Sync, Generate Cover, Manage Series, Quality Check,
Goodreads and Fantastic Fiction; FanFicFare, EpubMerge, EpubSplit; Comments
Cleaner; Ebook Translator). `scripts/fetch-plugins.py` downloads them and
`MAGNITIDE_PLUGIN_CORPUS=1 cargo test -p magnitide-plugins --test corpus`
installs each, loads the GUI ones in the host and writes
[calibre-plugins-coverage.md](calibre-plugins-coverage.md). As of
2026-10-07 every one loads except Ebook Translator, which needs calibre's
conversion pipeline. Loading is not the same as every feature working: the
plugins' own dialogs and jobs are exercised by hand (and by the fixture
plugins in `crates/plugins/tests/fixtures`, which do what the surveyed
plugins do).

### Developing

- `scripts/plugin-runtime/dev.sh` builds the runtime for this machine into
  `target/plugin-runtime/current` (needs Python 3.14 and uv);
  `scripts/plugin-runtime/fetch.sh` downloads a released one instead.
  `MAGNITIDE_PLUGIN_RUNTIME=<dir>` overrides where the app and tests look.
- `MAGNITIDE_COMPAT_DIR=crates/plugins/python` makes the app use the compat
  package from the source tree (no Rust rebuild for Python edits).
- `cargo test -p magnitide-plugins --test host` runs the host end to end
  with the fixture plugins (skipped without a runtime;
  `MAGNITIDE_REQUIRE_PLUGIN_RUNTIME=1` makes that a failure, as on CI).
  `MAGNITIDE_PLUGIN_HOST=calibre` runs the same tests inside an installed
  calibre (CI installs calibre's Linux build for it); the corpus honours it
  too and then writes `docs/calibre-plugins-coverage-calibre.md`.
- `crates/plugins/python-tests/` is the compat package's own pytest suite
  (`QT_QPA_PLATFORM=offscreen pytest crates/plugins/python-tests`, with the
  runtime's wheels on the path); `ruff` lints and formats the Python.
- A plugin that needs a Python package the runtime doesn't ship can have it
  installed into `<app config>/calibre-plugins/site-packages` (for example
  `python3.14 -m pip install --target <that folder> <package>`); the host
  adds that folder to its path. The runtime ships `pycryptodome` (plugins
  that sign or encrypt books import `Crypto`/`Cryptodome`) from version 2.
- Linux needs Qt's xcb libraries (see `scripts/plugin-runtime/README.md`);
  `python -I -m magnitide_host doctor` reports what's missing, as Settings
  does.

### The fallback: vendoring calibre

If the from-scratch package turns out to need more of calibre than is
reasonable to write, `scripts/vendor-calibre.py` (with
`scripts/vendor-calibre.toml`) copies the needed subset of calibre's own
GPL modules from a calibre checkout, replacing only the edges
(`constants`, `startup`, `customize.builtins`, `gui2.ui`, `gui2.device`,
`utils.icu`) and providing pure-Python stand-ins for calibre's C extensions
(`scripts/vendor-calibre/calibre_extensions/`). It writes a manifest with
every file's origin and hash. It isn't wired in: switching means running
it, deleting the compat modules it covers, and rerunning the tests;
reverting is a `git revert`.

## Status

1. **Host and metadata sources** (done 2026-10-06).
2. **Shadow library and window stand-in** (done 2026-10-06): see the design
   above; checked end to end with the fixture GUI plugin, which edits through
   both of calibre's database APIs, writes into a book file, sets a cover,
   adds a book, reports status, opens a dialog, hands over to Edit Metadata,
   Remove Books, Find Metadata and conversion, searches, views, and runs a
   job.
3. **Magnitide's own runtime and `calibre` package** (done 2026-10-07):
   replaced the installed-calibre host entirely; the plugin corpus above
   loads.
4. **The other plugin kinds** (done 2026-10-07): file-type, metadata
   reader/writer, preferences and library-closed plugins, with fixtures for
   each.

Still to do:

- Exercising the corpus plugins' own features (Count Pages into `#pages`,
  Comments Cleaner on comments, EpubSplit into a new book) as scripted
  scenarios rather than by hand.
- The template language's `program:` mode, and the rest of calibre's
  template functions.
- Creating custom columns in Magnitide for plugins that ask the user to
  (Count Pages): a column a plugin creates comes back, but there's no
  Magnitide UI to make one first.
- Incremental shadow updates: the shadow is rebuilt whole when the library
  changed, which is quick for thousands of books but not for 50,000.
- A plugin writing to a path it built itself (not from `format_abspath`)
  writes through the link into the library. None of the surveyed plugins
  do, but it isn't prevented.
- Windows: links need Developer Mode there; without it files are copied.
- The runtime release (`plugin-runtime-v1`, published 2026-10-07, hashes in
  `runtime.rs`) sits on a private repository, so the app's download gets a
  404 until the repository or the release is public. Meanwhile Settings ›
  Plugins › "Install from file…" installs the archive downloaded by hand
  from the release (only the one for this computer, by hash); `fetch.sh`
  and CI use `gh` with a token.

Better built natively than hosted, because they mostly drive calibre's
library view rather than doing work of their own:

- **Find Duplicates**: book duplicates by title and author at four strengths
  (identical, similar, soundex, fuzzy), by identifier (ISBN etc.), or by
  identical file; author, series, publisher and tag name variations;
  "not duplicates" exemptions; comparing two libraries. Magnitide's matching
  (`catalog::matching`), file fingerprints and Merge already cover much of it.
- **Reading List**: Magnitide's shelves plus device sync already do most of
  it; what's missing is per-list device rules (send to and remove from a
  device automatically).
