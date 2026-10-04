-- Magnitide library schema, version 1.
--
-- Conventions:
--   * Timestamps are UTC RFC 3339 text ("2026-10-03T18:04:05Z").
--   * Publication dates are ISO dates ("2021-11-23"), since most sources
--     only know the day.
--   * Names compare case-insensitively (COLLATE NOCASE) so "le guin" and
--     "Le Guin" are the same author.
--   * Library-wide data (titles, authors, files) is shared; personal data
--     (rating, reading status) is per user. The desktop app uses user 1,
--     "local"; the server will add real accounts.

-- Key/value facts about the library: its uuid, name, creation time.
CREATE TABLE library_info (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE users (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE authors (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- "Le Guin, Ursula K."
    sort_name TEXT NOT NULL COLLATE NOCASE
) STRICT;

CREATE TABLE series (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    sort_name TEXT NOT NULL COLLATE NOCASE
) STRICT;

CREATE TABLE publishers (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
) STRICT;

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
) STRICT;

CREATE TABLE books (
    id               INTEGER PRIMARY KEY,
    -- Stable identity across libraries, devices and exports.
    uuid             TEXT NOT NULL UNIQUE,
    title            TEXT NOT NULL,
    -- "Dispossessed, The"
    title_sort       TEXT NOT NULL COLLATE NOCASE,
    -- Folder relative to the library root, '/'-separated: "Author/Title (id)".
    path             TEXT NOT NULL UNIQUE,
    series_id        INTEGER REFERENCES series (id) ON DELETE SET NULL,
    series_index     REAL,
    publisher_id     INTEGER REFERENCES publishers (id) ON DELETE SET NULL,
    published        TEXT,
    -- Sanitized HTML.
    description_html TEXT,
    -- Cover image file name inside the book folder, if there is one.
    cover_file       TEXT,
    added_at         TEXT NOT NULL,
    modified_at      TEXT NOT NULL,
    CHECK (series_index IS NULL OR series_id IS NOT NULL)
) STRICT;

CREATE INDEX books_title_sort ON books (title_sort);
CREATE INDEX books_series ON books (series_id, series_index);
CREATE INDEX books_added ON books (added_at);

-- Authors in display order.
CREATE TABLE book_authors (
    book_id   INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors (id) ON DELETE RESTRICT,
    position  INTEGER NOT NULL,
    PRIMARY KEY (book_id, author_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX book_authors_author ON book_authors (author_id);

CREATE TABLE book_tags (
    book_id INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags (id) ON DELETE RESTRICT,
    PRIMARY KEY (book_id, tag_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX book_tags_tag ON book_tags (tag_id);

-- ISO 639-2 codes ("eng") in display order.
CREATE TABLE book_languages (
    book_id  INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    lang     TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (book_id, lang)
) STRICT, WITHOUT ROWID;

-- isbn, google, amazon, …
CREATE TABLE identifiers (
    book_id INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    scheme  TEXT NOT NULL COLLATE NOCASE,
    value   TEXT NOT NULL,
    PRIMARY KEY (book_id, scheme)
) STRICT, WITHOUT ROWID;

-- One file per format per book.
CREATE TABLE files (
    id       INTEGER PRIMARY KEY,
    book_id  INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    -- Upper case: "EPUB".
    format   TEXT NOT NULL,
    -- File name inside the book folder.
    filename TEXT NOT NULL,
    size     INTEGER NOT NULL,
    added_at TEXT NOT NULL,
    UNIQUE (book_id, format),
    UNIQUE (book_id, filename)
) STRICT;

-- Personal state, per user.
CREATE TABLE user_books (
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    book_id      INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    -- 1–10 (half stars), like calibre; NULL = not rated.
    rating       INTEGER CHECK (rating IS NULL OR rating BETWEEN 1 AND 10),
    status       TEXT NOT NULL DEFAULT 'unread' CHECK (status IN ('unread', 'reading', 'read')),
    -- 0.0–1.0 through the book.
    progress     REAL CHECK (progress IS NULL OR progress BETWEEN 0.0 AND 1.0),
    last_read_at TEXT,
    PRIMARY KEY (user_id, book_id)
) STRICT, WITHOUT ROWID;

-- Where an imported book came from, so imports can be repeated or reconciled.
CREATE TABLE book_sources (
    book_id        INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    -- 'calibre'
    source         TEXT NOT NULL,
    -- The source library's own id (calibre: library_id.uuid).
    source_library TEXT NOT NULL,
    -- The book's id in the source (calibre: books.id).
    source_book    TEXT NOT NULL,
    imported_at    TEXT NOT NULL,
    PRIMARY KEY (book_id, source),
    UNIQUE (source, source_library, source_book)
) STRICT, WITHOUT ROWID;

-- User-defined fields (calibre's custom columns map here).
CREATE TABLE custom_fields (
    id       INTEGER PRIMARY KEY,
    -- Lookup name without '#': "genre".
    key      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL CHECK (kind IN ('text', 'long_text', 'integer', 'number', 'bool', 'date', 'rating', 'choice', 'series')),
    -- Several values per book (e.g. a tag-like field).
    multiple INTEGER NOT NULL DEFAULT 0 CHECK (multiple IN (0, 1)),
    -- JSON: allowed choices, number format, …
    config   TEXT NOT NULL DEFAULT '{}'
) STRICT;

-- One row per value; `position` orders multi-valued fields. Exactly one of
-- text_value / num_value is used, depending on the field's kind (series
-- uses both: name and index).
CREATE TABLE custom_values (
    book_id    INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    field_id   INTEGER NOT NULL REFERENCES custom_fields (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL DEFAULT 0,
    text_value TEXT,
    num_value  REAL,
    PRIMARY KEY (book_id, field_id, position)
) STRICT, WITHOUT ROWID;

CREATE INDEX custom_values_field ON custom_values (field_id, text_value);
