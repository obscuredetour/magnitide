"""calibre's MOBI reader and writer aren't in Magnitide (metadata is read
and written by `calibre.ebooks.metadata.mobi`); this keeps the names
plugins import from the package."""


class MobiError(Exception):
    pass


MAX_THUMB_SIZE = 16 * 1024
MAX_THUMB_DIMEN = (180, 240)
