# Derived from calibre's src/calibre/db/write.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors the value adapters of calibre's `calibre.db.write`: turning what
a plugin passes into what a field's datatype holds."""

import re

from calibre.ebooks.metadata import author_to_author_sort
from calibre.utils.date import UNDEFINED_DATE, is_date_undefined, parse_date


def sqlite_datetime(x):
    from calibre.db.backend import dt_to_db

    return dt_to_db(x) if x is not None else None


def single_text(x):
    if x is None:
        return x
    if not isinstance(x, str):
        x = x.decode('utf-8', 'replace') if isinstance(x, bytes) else str(x)
    x = x.strip()
    return x if x else None


series_index_pat = re.compile(r'(.*)\s+\[([.0-9]+)\]$')


def get_series_values(val):
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


def multiple_text(sep, ui_sep, x):
    if not x:
        return ()
    if isinstance(x, bytes):
        x = x.decode('utf-8', 'replace')
    if isinstance(x, str):
        x = x.split(sep)
    x = (y.strip() if isinstance(y, str) else y for y in x)
    ui_sep = ui_sep.strip()
    repsep = ',' if ui_sep == ';' else ';'
    x = (y.replace(ui_sep, repsep) if isinstance(y, str) else y for y in x)
    return tuple(y for y in x if y)


def adapt_datetime(x):
    if isinstance(x, (str, bytes)):
        x = parse_date(x, assume_utc=False, as_utc=False)
    if x is not None and is_date_undefined(x):
        x = UNDEFINED_DATE
    return x


def adapt_date(x):
    if isinstance(x, (str, bytes)):
        x = parse_date(x, assume_utc=False, as_utc=False)
    if x is None or is_date_undefined(x):
        x = UNDEFINED_DATE
    return x


def adapt_number(typ, x):
    if x is None:
        return None
    if isinstance(x, (str, bytes)):
        if isinstance(x, bytes):
            x = x.decode('utf-8', 'replace')
        if not x or x.lower() == 'none':
            return None
    return typ(x)


def adapt_bool(x):
    if isinstance(x, (str, bytes)):
        if isinstance(x, bytes):
            x = x.decode('utf-8', 'replace')
        x = x.lower()
        if x == 'true':
            x = True
        elif x == 'false':
            x = False
        elif x == 'none' or x == '':
            x = None
        else:
            x = bool(int(x))
    return x


def adapt_languages(to_tuple, x):
    from calibre.utils.localization import canonicalize_lang

    ans = []
    for lang in to_tuple(x):
        lc = canonicalize_lang(lang)
        if not lc or lc in ans or lc in ('und', 'zxx', 'mis', 'mul'):
            continue
        ans.append(lc)
    return tuple(ans)


def clean_identifier(typ, val):
    typ = (typ or '').lower().strip().replace(':', '').replace(',', '')
    val = (val or '').strip().replace(',', '|')
    return typ, val


def adapt_identifiers(to_tuple, x):
    if not isinstance(x, dict):
        x = {k: v for k, v in (y.partition(':')[0::2] for y in to_tuple(x))}
    ans = {}
    for k, v in x.items():
        k, v = clean_identifier(k, v)
        if k and v:
            ans[k] = v
    return ans


def adapt_series_index(x):
    ret = adapt_number(float, x)
    return 1.0 if ret is None else ret


def get_adapter(name, metadata):
    dt = metadata['datatype']
    if dt == 'text':
        if metadata['is_multiple']:
            m = metadata['is_multiple']
            ans = lambda x: multiple_text(m['ui_to_list'], m['list_to_ui'], x)  # noqa: E731
        else:
            ans = single_text
    elif dt == 'series':
        ans = single_text
    elif dt == 'datetime':
        ans = adapt_date if name == 'pubdate' else adapt_datetime
    elif dt == 'int':
        ans = lambda x: adapt_number(int, x)  # noqa: E731
    elif dt == 'float':
        ans = lambda x: adapt_number(float, x)  # noqa: E731
    elif dt == 'bool':
        ans = adapt_bool
    elif dt == 'comments':
        ans = single_text
    elif dt == 'rating':
        ans = lambda x: None if x in {None, 0} else min(10, max(0, adapt_number(int, x)))  # noqa: E731
    elif dt == 'enumeration':
        ans = single_text
    elif dt == 'composite':
        ans = lambda x: x  # noqa: E731

    if name == 'title':
        return lambda x: ans(x) or 'Unknown'
    if name == 'author_sort':
        return lambda x: ans(x) or ''
    if name == 'authors':
        return lambda x: tuple(y.replace(',', '|') for y in ans(x)) or ('Unknown',)
    if name in {'timestamp', 'last_modified'}:
        return lambda x: ans(x) or UNDEFINED_DATE
    if name == 'series_index':
        return adapt_series_index
    if name == 'languages':
        return lambda x: adapt_languages(ans, x)
    if name == 'identifiers':
        return lambda x: adapt_identifiers(ans, x)
    return ans


__all__ = [
    'get_adapter',
    'get_series_values',
    'single_text',
    'multiple_text',
    'adapt_datetime',
    'adapt_date',
    'adapt_number',
    'adapt_bool',
    'adapt_languages',
    'adapt_identifiers',
    'adapt_series_index',
    'clean_identifier',
    'sqlite_datetime',
    'author_to_author_sort',
]
