# Derived from calibre's src/calibre/db/utils.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.utils`: helpers shared by the database
code and some plugins (`fuzzy_title`, `find_identical_books`)."""

import os
import re
import shutil
from contextlib import suppress

from calibre.utils.icu import lower as icu_lower

_fuzzy_pat = re.compile(r'[^\w\s]')
_strip_pat = re.compile(r'^(?:the|an?)\s+', re.IGNORECASE)


def fuzzy_title(title):
    """A title normalized for finding duplicates: lower case, leading
    articles and punctuation removed, whitespace collapsed."""
    if not title:
        return ''
    title = icu_lower(title).strip()
    title = _strip_pat.sub('', title)
    title = _fuzzy_pat.sub('', title)
    return ' '.join(title.split())


def find_identical_books(mi, data):
    """Books with the same fuzzy title and at least one author in common.
    `data` is what `Cache.data_for_find_identical_books()` gives."""
    author_map, titles = data
    if not mi.title:
        return set()
    ft = fuzzy_title(mi.title)
    candidates = {book_id for book_id, t in titles.items() if t == ft}
    return candidates


def atomic_rename(oldpath, newpath):
    os.replace(oldpath, newpath)


def rmtree_with_retry(path):
    shutil.rmtree(path, ignore_errors=True)


def remove_dir_if_empty(path):
    with suppress(OSError):
        os.rmdir(path)


class ThumbnailCache:
    """calibre's on-disk cover thumbnail cache; here a simple in-memory one."""

    def __init__(
        self,
        max_size=1024,
        name='thumbnail-cache',
        thumbnail_size=(100, 100),
        location=None,
        test_mode=False,
        min_disk_cache=0,
    ):
        self.cache = {}
        self.thumbnail_size = thumbnail_size

    def set_group_id(self, group_id):
        pass

    def set_thumbnail_size(self, width, height):
        self.thumbnail_size = (width, height)
        self.cache.clear()

    def invalidate(self, book_ids):
        for b in book_ids:
            self.cache.pop(b, None)

    def insert(self, book_id, timestamp, data):
        self.cache[book_id] = (timestamp, data)

    def __getitem__(self, book_id):
        return self.cache.get(book_id, (None, None))

    def __contains__(self, book_id):
        return book_id in self.cache

    def empty(self):
        self.cache.clear()

    def shutdown(self):
        pass

    def set_size(self, size):
        pass


__all__ = [
    'fuzzy_title',
    'find_identical_books',
    'atomic_rename',
    'rmtree_with_retry',
    'remove_dir_if_empty',
    'ThumbnailCache',
]
