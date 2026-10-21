-- Reading positions synced from e-readers (KOReader's progress sync) name a
-- book by a fingerprint of the file the reader opened, so the library keeps
-- its files' own fingerprints and those of the copies it hands out, to match
-- a position to its book (see positions.rs).

-- KOReader's fingerprint of the library's file (see fingerprint.rs), taken
-- when first needed, with the file's modification time at the time: stale,
-- and taken again, once that changes.
ALTER TABLE files ADD COLUMN fingerprint TEXT;
ALTER TABLE files ADD COLUMN fingerprint_mtime INTEGER;

-- Copies of a book's file that left the library: sent to a device, or
-- downloaded from the library server. Metadata written into the copy, or a
-- conversion on the way, gives it a fingerprint of its own.
CREATE TABLE file_copies (
    fingerprint TEXT NOT NULL,
    book_id     INTEGER NOT NULL REFERENCES books (id) ON DELETE CASCADE,
    format      TEXT NOT NULL,
    -- 'device' or 'server'.
    via         TEXT NOT NULL,
    -- The device's name, or what downloaded it ("KOReader", "Kobo").
    target      TEXT,
    at          TEXT NOT NULL,
    PRIMARY KEY (fingerprint, book_id)
) STRICT, WITHOUT ROWID;

-- Where the book was last read, as a synced position names it ("Kobo Libra
-- 2"); NULL when it was this app's own reader, or unknown.
ALTER TABLE user_books ADD COLUMN device TEXT;
