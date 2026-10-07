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

Reimplementing that surface would mean rebuilding much of calibre, so instead
the plugins run on calibre's own code, from the user's installed calibre, and
only the parts that touch the user's library and window are Magnitide's.

1. **Runtime: the user's calibre.** Magnitide already finds an installed
   calibre for conversion. A helper process runs under `calibre-debug -e`,
   which has calibre's Python, PyQt6 and every calibre module. Plugin zips load
   through calibre's own loader (`calibre.customize.zipplugin`), so
   `calibre_plugins.<name>` imports, `get_resources`, `get_icons`,
   `load_translations` and `JSONConfig` behave as in calibre. Magnitide bundles
   nothing of calibre, and works as before without it; only plugins need it.
2. **Library: a calibre-format shadow library.** Rather than imitate 70+
   database methods, give plugins a real calibre `LibraryDatabase`: Magnitide
   keeps a calibre-format copy of the open library's metadata (`metadata.db`,
   with the book files linked, not copied) in its cache, kept up to date
   incrementally. Plugins use calibre's real database code on it; when an
   action finishes, Magnitide reads back what changed (fields, custom
   columns, identifiers, covers, formats, new or removed books) and applies it
   through its normal edit path, so its own rules (renaming, write-back,
   undo-safety) still hold. The shadow is never one of the user's calibre
   libraries, which stay unwritten. This is the planned "export to calibre"
   writer, plus a change reader.
3. **Window: a stand-in for calibre's main window.** The helper gives each
   plugin a `gui` object implementing what the survey found:
   `library_view` (the selection is Magnitide's; `refresh_ids` /
   `books_added` / `current_changed` tell Magnitide what to reload),
   `status_bar.show_message` (Magnitide's status bar), `job_manager` (calibre's
   own job runner, reported as Magnitide jobs), `keyboard` (shortcuts recorded,
   not bound), `search` (Magnitide's search box), `iactions` (Edit Metadata,
   Remove Books and Convert Books mapped to Magnitide's; other hosted plugins
   as themselves), and inert stand-ins for the tag browser, cover flow and
   toolbar bars. Plugin menus appear in Magnitide (a Plugins menu and the book
   context menu); plugin dialogs open as their own Qt windows, so they look
   like calibre's.
4. **Simpler plugin types on the same host**: metadata sources join Find
   Metadata (calibre `Metadata` objects converted to Magnitide candidates);
   file-type plugins run on import.

Prerequisites in Magnitide itself: editing custom columns, and the
calibre-format writer.

## Plan

1. **Host and metadata sources** (done, 2026-10-06: `crates/plugins`).
   Checked with calibre 9.15 and a test plugin. Of kiwidude's sources,
   Goodreads still calls Goodreads' closed XML API, Barnes & Noble's search
   page has moved and FictionDB's markup has changed, so they find nothing
   inside calibre too; the host reports their own messages. The helper process, the plugin loader,
   Settings › Plugins (install a calibre plugin zip, enable, see its log), and
   metadata-source plugins in Find Metadata. No library access needed, so it
   proves the runtime first.
2. **Shadow library and window stand-in**, with the plugins that need least
   of the window: EmbedComicMetadata, EpubSplit, EpubMerge, then Count Pages
   (custom columns and jobs) and Comments Cleaner (the Edit Metadata mapping).
   In progress (2026-10-06). Working, and tested end to end with calibre 9.15
   and a test plugin (`crates/plugins/tests/fixtures/test_action`) that does
   what the surveyed plugins do:
   - **The shadow** (`plugins::shadow`): `calibre_compat::write` writes the
     open library's catalog as a `metadata.db` with the same book ids, uuids,
     folders and custom columns (it is the mirror of the reader, and calibre
     opens it); book folders hold links to the real files. 103 books build in
     under 0.1 s. Plugin settings stored in the library (`preferences`,
     `books_plugin_data`) survive rebuilds.
   - **Files are safe**: calibre's `add_format` and others write _through_ a
     link, so the host turns a link into a private copy (a clone on APFS,
     Btrfs, XFS) before calibre hands a plugin its path (`format_abspath`) or
     writes to it (`apply_to_format`, `add_format`, `set_cover`, hard-linking
     copies). Only inside shadow libraries.
   - **The window** (`host.py`): Qt starts on the host's main thread the first
     time a GUI plugin is used. Plugins load as calibre loads them (genesis,
     layout complete, initialization complete; `library_changed` on rebuild).
     `gui` has the book list (selection is Magnitide's; refreshes are noted),
     `current_db`, `status_bar` (Magnitide's toasts), calibre's own keyboard
     manager and job manager, `iactions` for hosted plugins, and inert
     stand-ins (logged on first use) for everything else. On macOS the host is
     an accessory app: no Dock icon, plugin windows come to the front, and the
     focus goes back to Magnitide when they close.
   - **Menus and running**: the book menu's Plugins submenu shows each
     plugin's menu as calibre would (filled the way the plugin fills it on
     show); an item runs on the selected books.
   - **Reading back**: when the plugin is idle (its action returned, its
     windows are closed, its jobs are done) Magnitide compares the shadow
     with the catalog it was written from and applies the difference through
     its own edit path (`core::external`): metadata, custom values, new
     custom columns, replaced/added/removed formats, covers, new books. Books
     a plugin removed are only deleted after Magnitide asks.

   Still to do in this step:
   - Stand-ins for calibre's own actions plugins call through `gui.iactions`
     (Edit Metadata, Remove Books, Convert Books, Choose Library, Add Books),
     mapped to Magnitide's; today they're inert and logged.
   - Trying the surveyed plugins themselves (their zips aren't in the repo).
   - Creating custom columns in Magnitide for plugins that ask the user to
     (Count Pages): today a column a plugin creates in the shadow comes back,
     but there's no Magnitide UI to make one first.
   - Incremental shadow updates: the shadow is rebuilt whole when the library
     changed, which is quick for thousands of books but not for 50,000.
   - A plugin writing to a path it built itself (not from `format_abspath`)
     writes through the link into the library. None of the surveyed plugins
     do, but it isn't prevented.
   - Windows: links need Developer Mode there; without it files are copied.

3. **The heavy ones**: Ebook Translator (conversion pipeline; mostly
   translates through online services, AI ones included, which is the user's
   choice of plugin rather than a Magnitide feature), FanFicFare (the most
   cross-plugin calls; it also has a standalone CLI that could back a native
   "add from URL"), Goodreads Sync (check that Goodreads still allows its
   sign-in first).

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
