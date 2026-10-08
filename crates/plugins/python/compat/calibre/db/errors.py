"""Mirrors calibre's `calibre.db.errors`."""


class NoSuchFormat(ValueError):
    pass


class NoSuchBook(KeyError):
    pass


class FormatNotFound(NoSuchFormat):
    pass
