"""Mirrors `get_available_formats_for_book` from calibre's
`calibre.gui2.convert.single`; the dialog itself is Magnitide's."""


def get_available_formats_for_book(db, book_id):
    available_formats = db.formats(book_id, index_is_id=True)
    if not available_formats:
        available_formats = ''
    return {x.lower() for x in available_formats.split(',') if x}


def get_input_format_for_book(db, book_id, pref=None):
    available = sorted(get_available_formats_for_book(db, book_id))
    if not available:
        raise ValueError(_('This book has no actual e-book files'))
    if pref and pref.lower() in available:
        return pref.lower(), available
    return available[0], available
