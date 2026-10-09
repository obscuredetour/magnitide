# Derived from calibre's src/calibre/db/adding.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.adding`: finding book files in folders
(what Add Books does with a folder)."""

import os
import re
from collections import defaultdict

from calibre.ebooks import BOOK_EXTENSIONS


def splitext(path):
    key, ext = os.path.splitext(path)
    return key, ext[1:].lower()


def formats_ok(formats):
    return len(formats) > 0


def path_ok(path):
    return not os.path.isdir(path) and os.access(path, os.R_OK)


def compile_rule(rule):
    mt = rule['match_type']
    if 'with' in mt:
        q = rule['query'].lower()
        if 'startswith' in mt:

            def func(filename):
                return filename.lower().startswith(q)
        else:

            def func(filename):
                return filename.lower().endswith(q)
    elif 'glob' in mt:
        import fnmatch

        pat = re.compile(fnmatch.translate(rule['query']), re.IGNORECASE)

        def func(filename):
            return pat.match(filename) is not None
    else:
        pat = re.compile(rule['query'], re.IGNORECASE)

        def func(filename):
            return pat.match(filename) is not None

    ans = func
    if mt.startswith('not_'):

        def ans(filename):
            return not func(filename)

    return ans, rule['action'] == 'add'


def filter_filename(compiled_rules, filename):
    for q, action in compiled_rules:
        if q(filename):
            return action


_metadata_extensions = None


def metadata_extensions():
    global _metadata_extensions
    if _metadata_extensions is None:
        _metadata_extensions = frozenset(BOOK_EXTENSIONS) | {'opf'}
    return _metadata_extensions


def listdir(root, sort_by_mtime=False):
    items = (os.path.join(root, x) for x in os.listdir(root))
    if sort_by_mtime:

        def safe_mtime(x):
            try:
                return os.path.getmtime(x)
            except OSError:
                return 0

        items = sorted(items, key=safe_mtime)
    for path in items:
        if path_ok(path):
            yield path


def find_books_in_directory(dirpath, single_book_per_directory, compiled_rules=(), listdir_impl=listdir):
    """Groups of files (one book each) in a folder: a list of lists of
    paths. One book per folder when `single_book_per_directory`, else
    files sharing a name (without extension) are one book."""
    dirpath = os.path.abspath(dirpath)
    book_extentions = metadata_extensions()
    if single_book_per_directory:
        formats = {}
        for path in listdir_impl(dirpath):
            key, ext = splitext(path)
            if ext in book_extentions and filter_filename(compiled_rules, os.path.basename(path)) is not False:
                formats[ext] = path
        if formats_ok(formats):
            yield list(formats.values())
    else:
        books = defaultdict(dict)
        for path in listdir_impl(dirpath, sort_by_mtime=True):
            key, ext = splitext(path)
            if ext in book_extentions and filter_filename(compiled_rules, os.path.basename(path)) is not False:
                books[icu_lower(key) if isinstance(key, str) else key][ext] = path

        for formats in books.values():
            if formats_ok(formats):
                yield list(formats.values())


def icu_lower(x):
    from calibre.utils.icu import lower

    return lower(x)


def create_format_map(formats):
    fmap = {}
    for fmt in formats:
        ext = os.path.splitext(fmt)[1][1:].upper()
        if ext == 'OPF':
            continue
        fmap[ext] = fmt
    return fmap


def import_book_directory_multiple(db, dirpath, callback=None, added_ids=None, compiled_rules=()):
    duplicates = []
    for formats in find_books_in_directory(dirpath, False, compiled_rules=compiled_rules):
        mi = _metadata_from_formats(formats)
        if mi.title is None:
            continue
        ids, dups = db.new_api.add_books([(mi, create_format_map(formats))], add_duplicates=False, run_hooks=True)
        if dups:
            duplicates.append((mi, formats))
        if added_ids is not None:
            added_ids.update(ids)
        if callable(callback) and callback(mi.title):
            break
    return duplicates


def import_book_directory(db, dirpath, callback=None, added_ids=None, compiled_rules=()):
    dirpath = os.path.abspath(dirpath)
    formats = None
    for formats in find_books_in_directory(dirpath, True, compiled_rules=compiled_rules):
        break
    if not formats:
        return
    mi = _metadata_from_formats(formats)
    if mi.title is None:
        return
    ids, dups = db.new_api.add_books([(mi, create_format_map(formats))], add_duplicates=False, run_hooks=True)
    if dups:
        return [(mi, formats)]
    if added_ids is not None:
        added_ids.update(ids)
    if callable(callback):
        callback(mi.title)


def _metadata_from_formats(formats):
    from calibre.ebooks.metadata.meta import metadata_from_formats

    return metadata_from_formats(list(formats))


def recursive_import(db, root, single_book_per_directory=True, callback=None, added_ids=None, compiled_rules=()):
    root = os.path.abspath(root)
    duplicates = []
    for dirpath in os.walk(root):
        res = (
            import_book_directory(db, dirpath[0], callback=callback, added_ids=added_ids, compiled_rules=compiled_rules)
            if single_book_per_directory
            else import_book_directory_multiple(
                db, dirpath[0], callback=callback, added_ids=added_ids, compiled_rules=compiled_rules
            )
        )
        if res is not None:
            duplicates.extend(res)
        if callable(callback) and callback(''):
            break
    return duplicates


def add_catalog(cache, path, title, dbapi=None):
    from calibre.ebooks.metadata.book.base import Metadata
    from calibre.utils.date import utcnow

    fmt = os.path.splitext(path)[1][1:].lower()
    with open(path, 'rb') as stream:
        mi = Metadata(title, ['calibre'])
        mi.tags = ['Catalog']
        mi.pubdate = mi.timestamp = utcnow()
        db_id = cache.create_book_entry(mi)
        cache.add_format(db_id, fmt, stream, dbapi=dbapi)
    return db_id


__all__ = [
    'find_books_in_directory',
    'compile_rule',
    'filter_filename',
    'listdir',
    'import_book_directory',
    'import_book_directory_multiple',
    'recursive_import',
    'create_format_map',
    'add_catalog',
    'metadata_extensions',
]
