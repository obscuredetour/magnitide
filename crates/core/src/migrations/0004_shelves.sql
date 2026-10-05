-- Shelves: named collections the user puts books on (by hand, or by drag and
-- drop). Magnitide only; they're also written to e-readers as collections.
CREATE TABLE shelves (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at TEXT NOT NULL
) STRICT;

-- Books on a shelf, in the order they were added.
CREATE TABLE book_shelves (
    shelf_id INTEGER NOT NULL REFERENCES shelves (id) ON DELETE CASCADE,
    book_id  INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    added_at TEXT NOT NULL,
    PRIMARY KEY (shelf_id, book_id)
) STRICT;
CREATE INDEX book_shelves_book ON book_shelves (book_id);
