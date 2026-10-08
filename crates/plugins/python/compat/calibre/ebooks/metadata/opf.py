"""Mirrors calibre's `calibre.ebooks.metadata.opf`: reading and writing
metadata of a bare OPF file."""

from calibre.ebooks.metadata.opf2 import OPF


def get_metadata(stream):
    return OPF(stream, populate_spine=False).to_book_metadata()


def set_metadata(stream, mi, apply_null=False, update_timestamp=False, force_identifiers=False, add_missing_cover=True):
    stream.seek(0)
    opf = OPF(stream, populate_spine=False)
    opf.smart_update(mi, apply_null=apply_null)
    if update_timestamp:
        from calibre.utils.date import utcnow

        opf.timestamp = utcnow()
    raw = opf.render()
    stream.seek(0)
    stream.truncate()
    stream.write(raw)
    stream.flush()
