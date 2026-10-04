-- Omnibus editions and books in several series (Magnitide only; e-readers and
-- calibre know one series with one number).

-- Where an omnibus's range ends: series_index 1, series_end 3 → "Books 1–3".
ALTER TABLE books ADD COLUMN series_end REAL;

-- Series beyond the main one (books.series_id), in order.
CREATE TABLE book_more_series (
    book_id    INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    series_id  INTEGER NOT NULL REFERENCES series (id),
    position   INTEGER NOT NULL,
    number     REAL NOT NULL,
    number_end REAL,
    PRIMARY KEY (book_id, series_id)
) STRICT;

-- An omnibus and the books it contains, in order.
CREATE TABLE book_collects (
    omnibus_id INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    book_id    INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    PRIMARY KEY (omnibus_id, book_id),
    CHECK (omnibus_id <> book_id)
) STRICT;
CREATE INDEX book_collects_book ON book_collects (book_id);
