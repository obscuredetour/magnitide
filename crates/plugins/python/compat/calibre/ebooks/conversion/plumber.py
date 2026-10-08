"""Not available: calibre's conversion pipeline. Magnitide converts books
through its own code and an installed calibre's ebook-convert, not through
this module."""

raise ImportError(
    "calibre's calibre.ebooks.conversion.plumber isn't available in Magnitide: it is calibre's conversion pipeline, which Magnitide doesn't run"
)
