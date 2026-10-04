-- When Magnitide last wrote the book's metadata into this file (or found it
-- already matching). The file is out of date when the book was modified
-- after this. NULL: never checked.
ALTER TABLE files ADD COLUMN metadata_written_at TEXT;
