"""Mirrors calibre's `calibre.db.lazy`: `ProxyMetadata`, a Metadata whose
fields come from the database as they're asked for (`db.get_proxy_metadata`)."""

import os

from calibre.ebooks.metadata.book.base import Metadata
from calibre.utils.date import UNDEFINED_DATE


class FormatMetadata(dict):
    def __init__(self, db, book_id, formats):
        dict.__init__(self)
        self._db = db
        self._book_id = book_id
        for fmt in formats:
            try:
                self[fmt] = db.format_metadata(book_id, fmt)
            except Exception:
                pass


class FormatsList(list):
    def __init__(self, formats, format_metadata):
        list.__init__(self, formats)
        self._format_metadata = format_metadata


class ProxyMetadata(Metadata):
    """A Metadata filled from the database on first use of each field;
    plugins use it like any Metadata (reading). It also offers the
    read-only attributes calibre adds: `formats`, `format_metadata`,
    `has_cover`, `id`, `size`."""

    def __init__(self, db, book_id, formatter=None):
        Metadata.__init__(self, None)
        full = db.get_metadata(book_id)
        object.__setattr__(self, '_data', object.__getattribute__(full, '_data'))
        for k, v in full.__dict__.items():
            if k not in ('_data', 'formatter', 'template_cache'):
                self.__dict__[k] = v
        self.__dict__['_db'] = db
        self.__dict__['_book_id'] = book_id
        self.__dict__['_cover_data'] = None

    @property
    def cover_data(self):
        if self.__dict__.get('_cover_data') is None:
            data = self._db.cover(self._book_id)
            self.__dict__['_cover_data'] = ('jpeg', data) if data else (None, None)
        return self.__dict__['_cover_data']

    @cover_data.setter
    def cover_data(self, val):
        self.__dict__['_cover_data'] = val

    @property
    def cover(self):
        path = self._db.field_for('path', self._book_id)
        if path and self._db.field_for('cover', self._book_id):
            p = os.path.join(self._db.library_path, path, 'cover.jpg')
            return p if os.path.exists(p) else None
        return None

    @cover.setter
    def cover(self, val):
        self.__dict__['_cover_path'] = val

    @property
    def has_cover(self):
        return bool(self._db.field_for('cover', self._book_id))

    @has_cover.setter
    def has_cover(self, val):
        pass

    @property
    def id(self):
        return self._book_id

    @id.setter
    def id(self, val):
        pass

    @property
    def last_modified(self):
        return self._db.field_for('last_modified', self._book_id, UNDEFINED_DATE)

    @last_modified.setter
    def last_modified(self, val):
        pass

    def get_user_metadata(self, field, make_copy=False):
        return Metadata.get_user_metadata(self, field, make_copy)

    def deepcopy(self, **kw):
        return self._db.get_metadata(self._book_id)

    def __repr__(self):
        return f'<ProxyMetadata {self.title!r} ({self._book_id})>'


__all__ = ['ProxyMetadata', 'FormatMetadata', 'FormatsList']
