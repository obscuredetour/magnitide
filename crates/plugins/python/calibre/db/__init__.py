"""Mirrors calibre's `calibre.db` package (calibre 9.15): the library
database as plugins see it. In Magnitide it opens the shadow library (a
calibre-format copy of the open library) with a stand-alone SQLite
implementation of calibre's API: `Cache` (the "new API", `db.new_api`) and
`LibraryDatabase` (the legacy API most plugins use)."""

SPOOL_SIZE = 30 * 1024 * 1024


def _get_next_series_num_for_list(series_indices, unwrap=True):
    from math import ceil, floor

    from calibre.utils.config_base import tweaks

    if not series_indices:
        if isinstance(tweaks['series_index_auto_increment'], (int, float)):
            return float(tweaks['series_index_auto_increment'])
        return 1.0
    if unwrap:
        series_indices = [x[0] for x in series_indices]
    if tweaks['series_index_auto_increment'] == 'next':
        return floor(series_indices[-1]) + 1
    if tweaks['series_index_auto_increment'] == 'first_free':
        for i in range(1, 10000):
            if i not in series_indices:
                return i
        # really shouldn't get here.
    if tweaks['series_index_auto_increment'] == 'next_free':
        for i in range(int(ceil(series_indices[0])), 10000):
            if i not in series_indices:
                return i
        # really shouldn't get here.
    if tweaks['series_index_auto_increment'] == 'last_free':
        for i in range(int(ceil(series_indices[-1])), 0, -1):
            if i not in series_indices:
                return i
        return series_indices[-1] + 1
    if isinstance(tweaks['series_index_auto_increment'], (int, float)):
        return float(tweaks['series_index_auto_increment'])
    return 1.0


def _get_series_values(val):
    import re

    series_index_pat = re.compile(r'(.*)\s+\[([.0-9]+)\]$')
    if not val:
        return (val, None)
    match = series_index_pat.match(val.strip())
    if match is not None:
        idx = match.group(2)
        try:
            idx = float(idx)
            return (match.group(1).strip(), idx)
        except Exception:
            pass
    return (val, None)


def get_data_as_dict(self, prefix=None, authors_as_string=False, ids=None, convert_to_local_tz=True):
    """Every book's metadata as a list of dicts (the legacy API's
    `get_data_as_dict`)."""

    from calibre.ebooks.metadata import authors_to_string
    from calibre.utils.date import as_local_time

    backend = getattr(self, 'backend', getattr(self, 'new_api', self))
    if prefix is None:
        prefix = backend.library_path
    fdata = backend.custom_column_num_map if hasattr(backend, 'custom_column_num_map') else {}
    FIELDS = {
        'title',
        'sort',
        'authors',
        'author_sort',
        'publisher',
        'rating',
        'timestamp',
        'size',
        'tags',
        'comments',
        'series',
        'series_index',
        'uuid',
        'pubdate',
        'last_modified',
        'identifiers',
        'languages',
    }.union(set(fdata))
    for x, data in fdata.items():
        if data['datatype'] == 'series':
            FIELDS.add('%d_index' % x)
    data = []
    api = getattr(self, 'new_api', self)
    all_ids = api.all_book_ids() if ids is None else ids
    for book_id in sorted(all_ids):
        mi = api.get_metadata(book_id)
        x = {}
        for field in FIELDS:
            if isinstance(field, int):
                field = '#' + fdata[field]['label']
            val = mi.get(field, None)
            if field in ('timestamp', 'pubdate', 'last_modified') and val is not None and convert_to_local_tz:
                val = as_local_time(val)
            x[field] = val
        x['id'] = book_id
        if authors_as_string:
            x['authors'] = authors_to_string(x['authors'])
        x['formats'] = []
        x['isbn'] = mi.isbn or ''
        for fmt in api.formats(book_id):
            path = api.format_abspath(book_id, fmt)
            if path:
                x['formats'].append(path)
        data.append(x)
    return data


__all__ = ['SPOOL_SIZE', '_get_next_series_num_for_list', '_get_series_values', 'get_data_as_dict']
