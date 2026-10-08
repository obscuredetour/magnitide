"""Mirrors calibre's `calibre.gui2.library` package (the book list). In
Magnitide the book list is the host's stand-in; `models.BooksModel` here is
what plugins importing the class get."""

DEFAULT_SORT = ('timestamp', False)


def sort_by_in_place(*a, **k):
    pass
