# Derived from calibre's src/calibre/db/cache.py (calibre 9.15).
# Copyright: 2011, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.cache` (calibre 9.15): `Cache`, the API
behind `db.new_api`. Every call reads or writes the shadow's SQLite
database directly (calibre caches tables in memory; the shadow is small
enough not to need it). Writes stamp the book's `last_modified`, which is
how Magnitide notices what a plugin changed.

Fields: the standard ones ('title', 'authors', 'series', 'series_index',
'tags', 'publisher', 'rating', 'languages', 'identifiers', 'comments',
'pubdate', 'timestamp', 'last_modified', 'uuid', 'sort', 'author_sort',
'path', 'cover', 'formats', 'size') and custom columns ('#name'). List
fields take lists; `identifiers` a dict; dates datetimes; `rating` 0–10.
"""

import os
import shutil
import threading
import traceback
from contextlib import contextmanager
from functools import wraps
from io import BytesIO

from calibre.db import _get_next_series_num_for_list, _get_series_values
from calibre.db.backend import DB, db_to_dt, dt_to_db
from calibre.db.errors import NoSuchBook, NoSuchFormat
from calibre.ebooks.metadata import author_to_author_sort, authors_to_string, string_to_authors, title_sort
from calibre.ebooks.metadata.book.base import Metadata
from calibre.utils.date import UNDEFINED_DATE, is_date_undefined, utcnow
from calibre.utils.icu import lower as icu_lower
from calibre.utils.icu import sort_key

_LINKED = {
    'authors': ('authors', 'name', 'books_authors_link', 'author', True),
    'tags': ('tags', 'name', 'books_tags_link', 'tag', True),
    'languages': ('languages', 'lang_code', 'books_languages_link', 'lang_code', True),
    'series': ('series', 'name', 'books_series_link', 'series', False),
    'publisher': ('publishers', 'name', 'books_publishers_link', 'publisher', False),
    'rating': ('ratings', 'rating', 'books_ratings_link', 'rating', False),
}
_BOOK_COLUMNS = {
    'title': 'title',
    'sort': 'sort',
    'author_sort': 'author_sort',
    'timestamp': 'timestamp',
    'pubdate': 'pubdate',
    'series_index': 'series_index',
    'path': 'path',
    'uuid': 'uuid',
    'last_modified': 'last_modified',
    'cover': 'has_cover',
}
_DATES = {'timestamp', 'pubdate', 'last_modified'}


def api(f):
    f.is_cache_api = True
    return f


read_api = write_api = api


class Cache:
    def __init__(self, backend, library_database_instance=None):
        self.backend = backend
        self.field_metadata = backend.field_metadata
        self._lock = threading.RLock()
        self.format_metadata_cache = {}
        self.dirtied_cache = {}
        self.vls_for_books_cache = None
        self.library_database_instance = library_database_instance
        self.formatter_template_cache = {}
        self.link_maps_cache = {}
        self.composites = frozenset()
        self.initialized = True
        self._search = None
        self.listeners = set()

    # -- Housekeeping --

    @property
    def conn(self):
        return self.backend.conn

    @property
    def prefs(self):
        return self.backend.prefs

    @property
    def library_id(self):
        return self.backend.library_id

    @property
    def dbpath(self):
        return self.backend.dbpath

    @property
    def library_path(self):
        return self.backend.library_path

    @property
    def is_closed(self):
        return self.backend.is_closed

    def close(self):
        self.backend.close()

    def reopen(self, force=False):
        self.backend.reopen(force)

    def refresh(self, *a, **k):
        pass

    def init(self):
        pass

    def commit(self):
        self.backend.commit()

    @contextmanager
    def read_lock(self):
        with self._lock:
            yield

    @contextmanager
    def write_lock(self):
        with self._lock:
            yield

    safe_read_lock = read_lock

    def add_listener(self, event_callback_function):
        self.listeners.add(event_callback_function)

    def remove_listener(self, event_callback_function):
        self.listeners.discard(event_callback_function)

    def _fire(self, event, *args):
        for f in list(self.listeners):
            try:
                f(event, self, *args)
            except Exception:
                traceback.print_exc()

    def _touch(self, book_ids):
        self.backend.update_last_modified(book_ids)

    # -- Reading --

    def all_book_ids(self, type=frozenset):
        return type(r[0] for r in self.conn.execute('SELECT id FROM books'))

    def has_id(self, book_id):
        return self.conn.execute('SELECT 1 FROM books WHERE id=?', (book_id,)).fetchone() is not None

    def _book_row(self, book_id):
        row = self.conn.execute(
            'SELECT id, title, sort, author_sort, timestamp, pubdate, series_index, path, uuid, has_cover, last_modified FROM books WHERE id=?',
            (book_id,),
        ).fetchone()
        return row

    def _linked(self, field, book_id):
        table, col, lt, lcol, multiple = _LINKED[field]
        order = (
            ' ORDER BY l.id'
            if field in ('authors', 'tags')
            else (' ORDER BY l.item_order' if field == 'languages' else '')
        )
        rows = self.conn.execute(
            f'SELECT v.{col} FROM {lt} l JOIN {table} v ON v.id = l.{lcol} WHERE l.book=?{order}', (book_id,)
        ).fetchall()
        vals = [r[0] for r in rows]
        if multiple:
            return tuple(vals)
        return vals[0] if vals else None

    def _custom_value(self, key, book_id):
        data = self.backend.custom_column_label_map[key[1:]]
        table, lt = self.backend.custom_table_names(data['num'])
        dt = data['datatype']
        if dt == 'composite':
            return None
        if data['normalized']:
            if dt == 'series':
                row = self.conn.execute(
                    f'SELECT v.value, l.extra FROM {lt} l JOIN {table} v ON v.id=l.value WHERE l.book=?', (book_id,)
                ).fetchone()
                return (row[0], row[1]) if row else (None, None)
            rows = self.conn.execute(
                f'SELECT v.value FROM {lt} l JOIN {table} v ON v.id=l.value WHERE l.book=? ORDER BY l.id', (book_id,)
            ).fetchall()
            vals = [r[0] for r in rows]
            if data['is_multiple']:
                return tuple(vals)
            if dt == 'rating':
                return vals[0] if vals else None
            return vals[0] if vals else None
        row = self.conn.execute(f'SELECT value FROM {table} WHERE book=?', (book_id,)).fetchone()
        if row is None:
            return None
        val = row[0]
        if dt == 'datetime':
            return db_to_dt(val)
        if dt == 'bool':
            return None if val is None else bool(val)
        return val

    def field_for(self, name, book_id, default_value=None):
        """A book's value for a field (see the module doc for the shapes)."""
        if name == 'id':
            return book_id if self.has_id(book_id) else default_value
        if name == 'title_sort':
            name = 'sort'
        if name in _LINKED:
            val = self._linked(name, book_id)
            if val is None or val == ():
                if name == 'authors':
                    return ('Unknown',) if self.has_id(book_id) else default_value
                return default_value if val is None else val
            return val
        if name in _BOOK_COLUMNS:
            row = self._book_row(book_id)
            if row is None:
                return default_value
            idx = {
                'title': 1,
                'sort': 2,
                'author_sort': 3,
                'timestamp': 4,
                'pubdate': 5,
                'series_index': 6,
                'path': 7,
                'uuid': 8,
                'cover': 9,
                'last_modified': 10,
            }[name]
            val = row[idx]
            if name in _DATES:
                return db_to_dt(val) or UNDEFINED_DATE
            if name == 'cover':
                return bool(val)
            return val if val is not None else default_value
        if name == 'comments':
            row = self.conn.execute('SELECT text FROM comments WHERE book=?', (book_id,)).fetchone()
            return row[0] if row else default_value
        if name == 'identifiers':
            return {k: v for k, v in self.conn.execute('SELECT type, val FROM identifiers WHERE book=?', (book_id,))}
        if name == 'formats':
            fm = tuple(
                r[0].upper() for r in self.conn.execute('SELECT format FROM data WHERE book=? ORDER BY id', (book_id,))
            )
            return fm if fm else default_value
        if name == 'size':
            row = self.conn.execute('SELECT MAX(uncompressed_size) FROM data WHERE book=?', (book_id,)).fetchone()
            return row[0] if row and row[0] is not None else default_value
        if name == 'au_map':
            return tuple(
                f'{n}:::{s or ""}:::{l or ""}'
                for n, s, l in self.conn.execute(
                    'SELECT a.name, a.sort, a.link FROM books_authors_link l JOIN authors a ON a.id=l.author WHERE l.book=? ORDER BY l.id',
                    (book_id,),
                )
            )
        if name == 'marked' or name == 'ondevice' or name == 'in_tag_browser':
            return default_value
        if name == 'series_sort':
            s = self._linked('series', book_id)
            return title_sort(s) if s else default_value
        if name.startswith('#'):
            if (
                name.endswith('_index')
                and name[:-6] in self.field_metadata
                and self.field_metadata[name[:-6]]['datatype'] == 'series'
            ):
                v = self._custom_value(name[:-6], book_id)
                return v[1] if v and v[0] is not None else default_value
            if name[1:] not in self.backend.custom_column_label_map:
                raise KeyError(f'No such field: {name}')
            dt = self.field_metadata[name]['datatype']
            v = self._custom_value(name, book_id)
            if dt == 'series':
                return v[0] if v and v[0] is not None else default_value
            if dt == 'composite':
                return self.get_proxy_metadata(book_id).get(name, default_value)
            if v is None or v == ():
                return default_value
            return v
        raise KeyError(f'No such field: {name}')

    fast_field_for = field_for

    def all_field_for(self, field, book_ids, default_value=None):
        return {book_id: self.field_for(field, book_id, default_value=default_value) for book_id in book_ids}

    def field_ids_for(self, name, book_id):
        """The item ids (in the field's table) linked to a book."""
        if name in _LINKED:
            table, col, lt, lcol, multiple = _LINKED[name]
            return tuple(
                r[0] for r in self.conn.execute(f'SELECT {lcol} FROM {lt} WHERE book=? ORDER BY id', (book_id,))
            )
        if name.startswith('#'):
            data = self.backend.custom_column_label_map[name[1:]]
            if data['normalized']:
                table, lt = self.backend.custom_table_names(data['num'])
                return tuple(
                    r[0] for r in self.conn.execute(f'SELECT value FROM {lt} WHERE book=? ORDER BY id', (book_id,))
                )
        return ()

    def _item_table(self, field):
        if field in _LINKED:
            table, col, lt, lcol, multiple = _LINKED[field]
            return table, col, lt, lcol
        if field.startswith('#'):
            data = self.backend.custom_column_label_map[field[1:]]
            if data['normalized']:
                table, lt = self.backend.custom_table_names(data['num'])
                return table, 'value', lt, 'value'
        return None

    def all_field_names(self, field):
        t = self._item_table(field)
        if t is None:
            return frozenset()
        return frozenset(r[0] for r in self.conn.execute(f'SELECT {t[1]} FROM {t[0]}'))

    def all_field_ids(self, field):
        t = self._item_table(field)
        if t is None:
            return frozenset()
        return frozenset(r[0] for r in self.conn.execute(f'SELECT id FROM {t[0]}'))

    def get_item_name(self, field, item_id):
        t = self._item_table(field)
        if t is None:
            return None
        row = self.conn.execute(f'SELECT {t[1]} FROM {t[0]} WHERE id=?', (item_id,)).fetchone()
        return row[0] if row else None

    def get_item_names(self, field, item_ids):
        return {i: self.get_item_name(field, i) for i in item_ids}

    def get_item_id(self, field, item_name, case_sensitive=False):
        t = self._item_table(field)
        if t is None:
            return None
        if field == 'rating':
            row = self.conn.execute(f'SELECT id FROM {t[0]} WHERE {t[1]}=?', (item_name,)).fetchone()
            return row[0] if row else None
        if case_sensitive:
            row = self.conn.execute(f'SELECT id FROM {t[0]} WHERE {t[1]}=? COLLATE BINARY', (item_name,)).fetchone()
        else:
            row = self.conn.execute(f'SELECT id FROM {t[0]} WHERE {t[1]}=? COLLATE NOCASE', (item_name,)).fetchone()
        return row[0] if row else None

    def get_item_ids(self, field, item_names, case_sensitive=False):
        return {n: self.get_item_id(field, n, case_sensitive) for n in item_names}

    def books_for_field(self, name, item_id):
        t = self._item_table(name)
        if t is None:
            return frozenset()
        return frozenset(r[0] for r in self.conn.execute(f'SELECT book FROM {t[2]} WHERE {t[3]}=?', (item_id,)))

    def get_usage_count_by_id(self, field):
        t = self._item_table(field)
        if t is None:
            return {}
        return {r[0]: r[1] for r in self.conn.execute(f'SELECT {t[3]}, COUNT(book) FROM {t[2]} GROUP BY {t[3]}')}

    def author_data(self, author_ids=None):
        rows = self.conn.execute('SELECT id, name, sort, link FROM authors')
        ans = {i: {'name': n, 'sort': s, 'link': l} for i, n, s, l in rows}
        if author_ids is not None:
            ans = {i: ans[i] for i in author_ids if i in ans}
        return ans

    def author_sort_from_authors(self, authors, key_func=None):
        ans = []
        for a in authors:
            row = self.conn.execute('SELECT sort FROM authors WHERE name=? COLLATE NOCASE', (a,)).fetchone()
            ans.append(row[0] if row and row[0] else author_to_author_sort(a))
        return ' & '.join(ans)

    def author_sort_strings_for_books(self, book_ids):
        ans = {}
        for book_id in book_ids:
            rows = self.conn.execute(
                'SELECT a.sort, a.name FROM books_authors_link l JOIN authors a ON a.id=l.author WHERE l.book=? ORDER BY l.id',
                (book_id,),
            )
            ans[book_id] = tuple(s or author_to_author_sort(n) for s, n in rows)
        return ans

    def get_link_map(self, for_field):
        t = self._item_table(for_field)
        if t is None:
            return {}
        try:
            return {n: l for n, l in self.conn.execute(f'SELECT {t[1]}, link FROM {t[0]} WHERE link != ""')}
        except Exception:
            return {}

    def get_all_link_maps_for_book(self, book_id):
        return {}

    def set_link_map(self, field, value_to_link_map, only_set_if_no_existing_link=False):
        return set()

    def field_supports_notes(self, field=None):
        return False

    def custom_field_keys(self, include_composites=True):
        return self.field_metadata.custom_field_keys(include_composites=include_composites)

    def last_modified(self):
        row = self.conn.execute('SELECT MAX(last_modified) FROM books').fetchone()
        return db_to_dt(row[0]) if row and row[0] else UNDEFINED_DATE

    def pref(self, name, default=None, namespace=None):
        if namespace is not None:
            return self.backend.prefs.get_namespaced(namespace, name, default)
        return self.backend.prefs.get(name, default)

    def set_pref(self, name, val, namespace=None):
        if namespace is not None:
            self.backend.prefs.set_namespaced(namespace, name, val)
        else:
            self.backend.prefs.set(name, val)

    def get_metadata(self, book_id, get_cover=False, get_user_categories=True, cover_as_data=False):
        """A `Metadata` for the book, custom columns included; `get_cover`
        adds the cover's path (or its bytes, `cover_as_data`)."""
        if not self.has_id(book_id):
            raise NoSuchBook(book_id)
        mi = Metadata(None, template_cache=self.formatter_template_cache)
        mi.title = self.field_for('title', book_id, 'Unknown')
        mi.authors = list(self.field_for('authors', book_id, ('Unknown',)))
        mi.author_sort = self.field_for('author_sort', book_id) or self.author_sort_from_authors(mi.authors)
        mi.title_sort = self.field_for('sort', book_id) or title_sort(mi.title)
        for field in ('series', 'publisher', 'comments', 'uuid', 'path'):
            setattr(mi, field, self.field_for(field, book_id))
        mi.series_index = self.field_for('series_index', book_id, 1.0) if mi.series else None
        if mi.series_index is None and mi.series:
            mi.series_index = 1.0
        mi.rating = self.field_for('rating', book_id)
        mi.tags = list(self.field_for('tags', book_id, ()))
        mi.languages = list(self.field_for('languages', book_id, ()))
        mi.set_identifiers(self.field_for('identifiers', book_id, {}))
        for field in ('timestamp', 'pubdate', 'last_modified'):
            setattr(mi, field, self.field_for(field, book_id, UNDEFINED_DATE))
        mi.formats = list(self.field_for('formats', book_id, ()) or ())
        mi.size = self.field_for('size', book_id)
        mi.application_id = book_id
        mi.id = book_id
        mi.db_id = book_id
        mi.has_cover = bool(self.field_for('cover', book_id))
        mi.author_sort_map = {}
        for n, s in self.conn.execute(
            'SELECT a.name, a.sort FROM books_authors_link l JOIN authors a ON a.id=l.author WHERE l.book=?', (book_id,)
        ):
            mi.author_sort_map[n] = s or author_to_author_sort(n)
        mi.author_link_map = {}
        mi.format_metadata = {}
        for fmt in mi.formats:
            try:
                mi.format_metadata[fmt] = self.format_metadata(book_id, fmt)
            except Exception:
                pass
        for key in self.custom_field_keys():
            meta = self.field_metadata[key]
            dt = meta['datatype']
            m = dict(meta)
            if dt == 'composite':
                m['#value#'] = None
                mi.set_user_metadata(key, m)
                continue
            raw = self._custom_value(key, book_id)
            extra = None
            if dt == 'series':
                raw, extra = raw if raw else (None, None)
            elif dt == 'text' and meta['is_multiple']:
                raw = list(raw) if raw else []
            m['#value#'] = raw
            m['#extra#'] = extra
            mi.set_user_metadata(key, m)
        if get_cover and mi.has_cover:
            if cover_as_data:
                mi.cover_data = ('jpeg', self.cover(book_id))
            else:
                mi.cover = self.cover(book_id, as_path=True)
        if get_user_categories:
            mi.user_categories = {}
        return mi

    def get_proxy_metadata(self, book_id):
        from calibre.db.lazy import ProxyMetadata

        return ProxyMetadata(self, book_id)

    def get_categories(self, sort='name', book_ids=None, first_letter_sort=False):
        from calibre.db.categories import get_categories

        return get_categories(self, sort=sort, book_ids=book_ids, first_letter_sort=first_letter_sort)

    # -- Formats --

    def _format_row(self, book_id, fmt):
        return self.conn.execute('SELECT name FROM data WHERE book=? AND format=?', (book_id, fmt.upper())).fetchone()

    def formats(self, book_id, verify_formats=True):
        fmts = self.field_for('formats', book_id, ()) or ()
        if not verify_formats:
            return fmts
        path = self.field_for('path', book_id)
        ans = []
        for fmt in fmts:
            row = self._format_row(book_id, fmt)
            if row and self.backend.has_format(book_id, fmt, row[0], path):
                ans.append(fmt)
        return tuple(ans)

    def has_format(self, book_id, fmt, verify=True):
        return fmt.upper() in self.formats(book_id, verify_formats=verify)

    def format_abspath(self, book_id, fmt):
        row = self._format_row(book_id, fmt)
        path = self.field_for('path', book_id)
        if row is None or path is None:
            return None
        return self.backend.format_abspath(book_id, fmt, row[0], path)

    def format_files(self, book_id):
        return {
            name: fmt.upper()
            for fmt, name in self.conn.execute('SELECT format, name FROM data WHERE book=?', (book_id,))
        }

    def format_metadata(self, book_id, fmt, allow_cache=True, update_db=False):
        row = self._format_row(book_id, fmt)
        path = self.field_for('path', book_id)
        if row is None or path is None:
            return {}
        return self.backend.format_metadata(book_id, fmt, row[0], path)

    def format_hash(self, book_id, fmt):
        row = self._format_row(book_id, fmt)
        path = self.field_for('path', book_id)
        if row is None:
            raise NoSuchFormat('Record %d has no fmt: %s' % (book_id, fmt))
        return self.backend.format_hash(book_id, fmt, row[0], path)

    def format(self, book_id, fmt, as_file=False, as_path=False, preserve_filename=False):
        """The format's bytes, a file object (`as_file`), or a temporary
        path (`as_path`)."""
        path = self.format_abspath(book_id, fmt)
        if path is None:
            return None
        if as_path:
            from calibre.ptempfile import PersistentTemporaryFile

            if preserve_filename:
                from calibre.ptempfile import PersistentTemporaryDirectory

                d = PersistentTemporaryDirectory('_format')
                dest = os.path.join(d, os.path.basename(path))
            else:
                pt = PersistentTemporaryFile('.' + fmt.lower())
                pt.close()
                dest = pt.name
            shutil.copyfile(path, dest)
            return dest
        if as_file:
            return open(path, 'rb')
        with open(path, 'rb') as f:
            return f.read()

    def copy_format_to(self, book_id, fmt, dest, use_hardlink=False, report_file_size=None):
        row = self._format_row(book_id, fmt)
        path = self.field_for('path', book_id)
        if row is None or path is None:
            raise NoSuchFormat('Record %d has no fmt: %s' % (book_id, fmt))
        return self.backend.copy_format_to(
            book_id, fmt, row[0], path, dest, use_hardlink=use_hardlink, report_file_size=report_file_size
        )

    def add_format(self, book_id, fmt, stream_or_path, replace=True, run_hooks=True, dbapi=None):
        """Add a file as the book's `fmt` (replacing an existing one if
        `replace`). Returns whether it was added."""
        fmt = (fmt or '').upper()
        if not self.has_id(book_id):
            raise NoSuchBook(book_id)
        if not replace and self.has_format(book_id, fmt):
            return False
        if run_hooks:
            from calibre.customize.ui import run_plugins_on_import

            if isinstance(stream_or_path, str):
                stream_or_path = run_plugins_on_import(stream_or_path, fmt.lower()) or stream_or_path
        path = self.field_for('path', book_id)
        title = self.field_for('title', book_id, 'Unknown')
        author = (self.field_for('authors', book_id, ('Unknown',)) or ('Unknown',))[0]
        current = self._format_row(book_id, fmt)
        current_name = current[0] if current else None
        stream = open(stream_or_path, 'rb') if isinstance(stream_or_path, str) else stream_or_path
        try:
            if current_name:
                # Keep the existing file name (Magnitide's naming).
                fname = current_name
                dest = os.path.join(self.library_path, path, fname + '.' + fmt.lower())
                os.makedirs(os.path.dirname(dest), exist_ok=True)
                if os.path.islink(dest):
                    os.remove(dest)
                with open(dest, 'wb') as f:
                    shutil.copyfileobj(stream, f)
                size = os.path.getsize(dest)
            else:
                size, fname = self.backend.add_format(book_id, fmt, stream, title, author, path, current_name)
        finally:
            if stream is not stream_or_path:
                stream.close()
        with self.conn:
            self.conn.execute(
                'INSERT OR REPLACE INTO data (book, format, uncompressed_size, name) VALUES (?,?,?,?)',
                (book_id, fmt, size, fname),
            )
            self._touch([book_id])
        if run_hooks:
            from calibre.customize.ui import run_plugins_on_postimport

            run_plugins_on_postimport(dbapi or self, book_id, fmt)
        self._fire('formats_added', {book_id: (fmt,)})
        return True

    def remove_formats(self, formats_map, db_only=False):
        """{book_id: [fmt, …]}; the files are deleted unless `db_only`."""
        removes = {}
        for book_id, fmts in formats_map.items():
            path = self.field_for('path', book_id)
            for fmt in fmts:
                row = self._format_row(book_id, fmt)
                if row is not None:
                    removes.setdefault(book_id, []).append((fmt.upper(), row[0], path))
        if not db_only:
            self.backend.remove_formats(removes)
        with self.conn:
            for book_id, items in removes.items():
                for fmt, fname, path in items:
                    self.conn.execute('DELETE FROM data WHERE book=? AND format=?', (book_id, fmt))
            self._touch(list(removes))
        return {book_id: tuple(f for f, _, _ in items) for book_id, items in removes.items()}

    def rename_format_files(self, *a, **k):
        pass

    # -- Covers --

    def cover(self, book_id, as_file=False, as_image=False, as_path=False, as_pixmap=False):
        path = self.field_for('path', book_id)
        if path is None:
            return None
        cpath = self.backend.cover_abspath(book_id, path)
        if cpath is None:
            return None
        if as_path:
            from calibre.ptempfile import PersistentTemporaryFile

            pt = PersistentTemporaryFile('_cover.jpg')
            pt.close()
            shutil.copyfile(cpath, pt.name)
            return pt.name
        if as_file:
            return open(cpath, 'rb')
        with open(cpath, 'rb') as f:
            data = f.read()
        if as_image or as_pixmap:
            from qt.core import QImage, QPixmap

            img = QPixmap() if as_pixmap else QImage()
            img.loadFromData(data)
            return img
        return data

    def cover_or_cache(self, book_id, timestamp, as_what='bytes'):
        return self.cover(book_id)

    def copy_cover_to(self, book_id, dest, use_hardlink=False, report_file_size=None):
        path = self.field_for('path', book_id)
        if path is None:
            return False
        return self.backend.copy_cover_to(path, dest, use_hardlink=use_hardlink, report_file_size=report_file_size)

    def cover_last_modified(self, book_id):
        path = self.field_for('path', book_id)
        return self.backend.cover_last_modified(path) if path else None

    def set_cover(self, book_id_data_map):
        """{book_id: bytes | file | QImage | None} (None removes the cover)."""
        for book_id, data in book_id_data_map.items():
            path = self.field_for('path', book_id)
            if path is None:
                continue
            self.backend.set_cover(book_id, path, data)
            with self.conn:
                self.conn.execute('UPDATE books SET has_cover=? WHERE id=?', (1 if data is not None else 0, book_id))
                self._touch([book_id])
        self._fire('cover_changed', tuple(book_id_data_map))
        return set(book_id_data_map)

    def has_cover(self, book_id):
        return bool(self.field_for('cover', book_id))

    # -- Writing --

    def _set_linked(self, field, book_id, values):
        table, col, lt, lcol, multiple = _LINKED[field]
        if not multiple:
            values = [values] if values not in (None, '', 0) or (field == 'rating' and values == 0 and False) else []
        else:
            values = [v for v in (values or []) if v not in (None, '')]
        self.conn.execute(f'DELETE FROM {lt} WHERE book=?', (book_id,))
        for i, v in enumerate(values):
            if field == 'rating':
                v = int(v)
                if v <= 0:
                    continue
                row = self.conn.execute(f'SELECT id FROM {table} WHERE rating=?', (v,)).fetchone()
                if row is None:
                    self.conn.execute(f'INSERT INTO {table} (rating) VALUES (?)', (v,))
                    item_id = self.backend.last_insert_rowid()
                else:
                    item_id = row[0]
            else:
                v = str(v).strip()
                if not v:
                    continue
                row = self.conn.execute(f'SELECT id FROM {table} WHERE {col}=? COLLATE NOCASE', (v,)).fetchone()
                if row is None:
                    if field == 'authors':
                        self.conn.execute(
                            f'INSERT INTO {table} (name, sort) VALUES (?, ?)', (v, author_to_author_sort(v))
                        )
                    elif field == 'series':
                        self.conn.execute(f'INSERT INTO {table} (name, sort) VALUES (?, ?)', (v, title_sort(v)))
                    else:
                        self.conn.execute(f'INSERT INTO {table} ({col}) VALUES (?)', (v,))
                    item_id = self.backend.last_insert_rowid()
                else:
                    item_id = row[0]
            if field == 'languages':
                self.conn.execute(
                    f'INSERT OR IGNORE INTO {lt} (book, {lcol}, item_order) VALUES (?, ?, ?)', (book_id, item_id, i)
                )
            else:
                self.conn.execute(f'INSERT OR IGNORE INTO {lt} (book, {lcol}) VALUES (?, ?)', (book_id, item_id))

    def _set_custom(self, key, book_id, val, extra=None):
        data = self.backend.custom_column_label_map[key[1:]]
        table, lt = self.backend.custom_table_names(data['num'])
        dt = data['datatype']
        if dt == 'composite':
            return
        if data['normalized']:
            self.conn.execute(f'DELETE FROM {lt} WHERE book=?', (book_id,))
            if dt == 'series':
                if isinstance(val, (tuple, list)) and len(val) == 2 and not isinstance(val[0], (int, float)):
                    val, extra = val
                else:
                    sval, sidx = _get_series_values(val) if isinstance(val, str) else (val, None)
                    if sidx is not None:
                        val, extra = sval, sidx
            if dt == 'rating':
                vals = [] if val in (None, '', 0) else [int(val)]
            elif data['is_multiple']:
                if isinstance(val, str):
                    val = [x.strip() for x in val.split(data['multiple_seps'].get('ui_to_list', ','))]
                vals = [str(v).strip() for v in (val or []) if str(v).strip()]
            else:
                vals = [] if val in (None, '') else [str(val).strip() if not isinstance(val, (int, float)) else val]
                vals = [v for v in vals if v != '']
            for v in vals:
                row = self.conn.execute(
                    f'SELECT id FROM {table} WHERE value=?' + (' COLLATE NOCASE' if dt not in ('rating',) else ''), (v,)
                ).fetchone()
                if row is None:
                    self.conn.execute(f'INSERT INTO {table} (value) VALUES (?)', (v,))
                    item_id = self.backend.last_insert_rowid()
                else:
                    item_id = row[0]
                if dt == 'series':
                    self.conn.execute(
                        f'INSERT OR IGNORE INTO {lt} (book, value, extra) VALUES (?, ?, ?)',
                        (book_id, item_id, float(extra) if extra is not None else 1.0),
                    )
                else:
                    self.conn.execute(f'INSERT OR IGNORE INTO {lt} (book, value) VALUES (?, ?)', (book_id, item_id))
            return
        self.conn.execute(f'DELETE FROM {table} WHERE book=?', (book_id,))
        if val is None or (isinstance(val, str) and not val.strip() and dt != 'comments'):
            return
        if dt == 'datetime':
            if isinstance(val, str):
                from calibre.utils.date import parse_date

                val = parse_date(val)
            if is_date_undefined(val):
                return
            val = dt_to_db(val)
        elif dt == 'bool':
            val = 1 if val else 0
        elif dt == 'int':
            val = int(val)
        elif dt == 'float':
            val = float(val)
        else:
            val = str(val)
        self.conn.execute(f'INSERT INTO {table} (book, value) VALUES (?, ?)', (book_id, val))

    def set_field(self, name, book_id_to_val_map, allow_case_change=True, do_path_update=True):
        """Set one field on several books: {book_id: value}. Returns the ids changed."""
        if name == 'title_sort':
            name = 'sort'
        if (
            name.startswith('#')
            and name.endswith('_index')
            and name[:-6] in self.field_metadata
            and self.field_metadata[name[:-6]]['datatype'] == 'series'
        ):
            changed = set()
            with self.conn:
                for book_id, val in book_id_to_val_map.items():
                    cur = self._custom_value(name[:-6], book_id)
                    if cur and cur[0] is not None:
                        self._set_custom(name[:-6], book_id, cur[0], extra=val)
                        changed.add(book_id)
                self._touch(changed)
            return changed
        changed = set()
        with self.conn:
            for book_id, val in book_id_to_val_map.items():
                if not self.has_id(book_id):
                    continue
                if name in _LINKED:
                    if name == 'authors':
                        if isinstance(val, str):
                            val = string_to_authors(val)
                        val = [a.strip() for a in (val or []) if a and a.strip()] or ['Unknown']
                    elif name in ('tags', 'languages'):
                        if isinstance(val, str):
                            val = [x.strip() for x in val.split(',')]
                        val = [v for v in (val or []) if v]
                    elif name == 'series' and isinstance(val, str):
                        sval, sidx = _get_series_values(val)
                        if sidx is not None:
                            val = sval
                            self.conn.execute('UPDATE books SET series_index=? WHERE id=?', (sidx, book_id))
                    self._set_linked(name, book_id, val)
                    if name == 'authors':
                        self.conn.execute(
                            'UPDATE books SET author_sort=? WHERE id=?', (self.author_sort_from_authors(val), book_id)
                        )
                elif name == 'title':
                    val = (val or '').strip() or 'Unknown'
                    self.conn.execute('UPDATE books SET title=?, sort=? WHERE id=?', (val, title_sort(val), book_id))
                elif name in ('sort', 'author_sort', 'uuid', 'path'):
                    self.conn.execute(f'UPDATE books SET {name}=? WHERE id=?', (val, book_id))
                elif name == 'series_index':
                    try:
                        val = float(val) if val is not None else 1.0
                    except Exception:
                        val = 1.0
                    self.conn.execute('UPDATE books SET series_index=? WHERE id=?', (val, book_id))
                elif name in _DATES:
                    if isinstance(val, str):
                        from calibre.utils.date import parse_date

                        val = parse_date(val)
                    if val is None or is_date_undefined(val):
                        val = UNDEFINED_DATE
                    self.conn.execute(f'UPDATE books SET {name}=? WHERE id=?', (dt_to_db(val), book_id))
                elif name == 'comments':
                    self.conn.execute('DELETE FROM comments WHERE book=?', (book_id,))
                    if val:
                        self.conn.execute('INSERT INTO comments (book, text) VALUES (?, ?)', (book_id, val))
                elif name == 'identifiers':
                    self.conn.execute('DELETE FROM identifiers WHERE book=?', (book_id,))
                    for k, v in (val or {}).items():
                        k, v = icu_lower(str(k)).strip(), str(v).strip()
                        if k and v:
                            self.conn.execute(
                                'INSERT OR REPLACE INTO identifiers (book, type, val) VALUES (?,?,?)', (book_id, k, v)
                            )
                elif name == 'cover':
                    self.conn.execute('UPDATE books SET has_cover=? WHERE id=?', (1 if val else 0, book_id))
                elif name.startswith('#'):
                    if name[1:] not in self.backend.custom_column_label_map:
                        raise KeyError(f'No such field: {name}')
                    self._set_custom(name, book_id, val)
                elif name in ('formats', 'size', 'id', 'marked', 'ondevice'):
                    continue
                else:
                    raise KeyError(f'Cannot set field: {name}')
                changed.add(book_id)
            if name != 'last_modified':
                self._touch(changed)
        if changed:
            self._fire('metadata_changed', name, changed)
        return changed

    def set_metadata(
        self,
        book_id,
        mi,
        ignore_errors=False,
        force_changes=False,
        set_title=True,
        set_authors=True,
        allow_case_change=False,
    ):
        """Set every non-null field of `mi` on the book (null fields too,
        when `force_changes`)."""
        if not self.has_id(book_id):
            raise NoSuchBook(book_id)

        def protected_set_field(name, val):
            try:
                self.set_field(name, {book_id: val})
            except Exception:
                if ignore_errors:
                    traceback.print_exc()
                else:
                    raise

        unknown = 'Unknown'
        if set_title and mi.title and mi.title != unknown:
            protected_set_field('title', mi.title)
        if set_authors:
            authors = list(mi.authors or [])
            if authors and authors != [unknown]:
                protected_set_field('authors', authors)
        for field in ('rating', 'series_index', 'timestamp', 'pubdate'):
            val = getattr(mi, field, None)
            if field == 'series_index':
                if mi.series or force_changes:
                    protected_set_field(field, val if val is not None else 1.0)
                continue
            if val is not None and (not mi.is_null(field) or force_changes):
                if field in ('timestamp', 'pubdate') and is_date_undefined(val) and not force_changes:
                    continue
                protected_set_field(field, val)
        for field in ('author_sort', 'publisher', 'series', 'comments', 'title_sort'):
            val = getattr(mi, field, None)
            if val or force_changes:
                if field == 'title_sort':
                    field = 'sort'
                protected_set_field(field, val if val else None)
        for field in ('tags', 'languages'):
            val = getattr(mi, field, None)
            if val or force_changes:
                protected_set_field(field, list(val or []))
        try:
            ids = mi.get_identifiers() if hasattr(mi, 'get_identifiers') else {}
        except Exception:
            ids = {}
        if ids or force_changes:
            current = self.field_for('identifiers', book_id, {}) if not force_changes else {}
            current.update(ids)
            protected_set_field('identifiers', current)
        if getattr(mi, 'uuid', None) and force_changes:
            protected_set_field('uuid', mi.uuid)
        if hasattr(mi, 'custom_field_keys'):
            for key in mi.custom_field_keys():
                if key not in self.field_metadata:
                    continue
                meta = mi.get_user_metadata(key, make_copy=False) or {}
                if meta.get('datatype') == 'composite':
                    continue
                val = mi.get(key, None)
                extra = mi.get_extra(key, None) if meta.get('datatype') == 'series' else None
                if val is None and not force_changes:
                    continue
                if meta.get('datatype') == 'series' and val:
                    protected_set_field(key, (val, extra if extra is not None else 1.0))
                else:
                    protected_set_field(key, val)
        cover = getattr(mi, 'cover_data', None)
        if cover and cover[1]:
            self.set_cover({book_id: cover[1]})
        elif getattr(mi, 'cover', None) and os.path.exists(mi.cover):
            with open(mi.cover, 'rb') as f:
                self.set_cover({book_id: f.read()})

    # -- Adding and removing books --

    def create_book_entry(
        self, mi, cover=None, add_duplicates=True, force_id=None, apply_import_tags=True, preserve_uuid=False
    ):
        """A new book record from `mi` (no formats). Returns its id, or None
        for a duplicate when `add_duplicates` is False."""
        if mi.tags is None:
            mi.tags = []
        if apply_import_tags:
            from calibre.utils.config import prefs as cprefs

            mi.tags = list(mi.tags) + list(cprefs['new_book_tags'] or [])
        if not add_duplicates and self.has_book(mi):
            return None
        title = (mi.title or 'Unknown').strip() or 'Unknown'
        authors = [a for a in (mi.authors or []) if a] or ['Unknown']
        import uuid as _uuid

        book_uuid = (mi.uuid if preserve_uuid and getattr(mi, 'uuid', None) else None) or str(_uuid.uuid4())
        ts = (
            mi.timestamp
            if getattr(mi, 'timestamp', None) is not None and not is_date_undefined(mi.timestamp)
            else utcnow()
        )
        pd = (
            mi.pubdate
            if getattr(mi, 'pubdate', None) is not None and not is_date_undefined(mi.pubdate)
            else UNDEFINED_DATE
        )
        si = float(mi.series_index) if mi.series and mi.series_index is not None else 1.0
        with self.conn:
            if force_id is None:
                self.conn.execute(
                    'INSERT INTO books(title, sort, series_index, author_sort, timestamp, pubdate, uuid, last_modified, path) VALUES (?,?,?,?,?,?,?,?,?)',
                    (
                        title,
                        title_sort(title),
                        si,
                        self.author_sort_from_authors(authors),
                        dt_to_db(ts),
                        dt_to_db(pd),
                        book_uuid,
                        dt_to_db(utcnow()),
                        '',
                    ),
                )
                book_id = self.backend.last_insert_rowid()
            else:
                self.conn.execute(
                    'INSERT INTO books(id, title, sort, series_index, author_sort, timestamp, pubdate, uuid, last_modified, path) VALUES (?,?,?,?,?,?,?,?,?,?)',
                    (
                        force_id,
                        title,
                        title_sort(title),
                        si,
                        self.author_sort_from_authors(authors),
                        dt_to_db(ts),
                        dt_to_db(pd),
                        book_uuid,
                        dt_to_db(utcnow()),
                        '',
                    ),
                )
                book_id = force_id
            path = self.backend.construct_path_name(book_id, title, authors[0])
            self.conn.execute('UPDATE books SET path=? WHERE id=?', (path, book_id))
            self._set_linked('authors', book_id, authors)
        mi2 = mi.deepcopy_metadata() if hasattr(mi, 'deepcopy_metadata') else mi
        mi2.title = title
        mi2.authors = authors
        self.set_metadata(book_id, mi2, ignore_errors=True, set_title=False, set_authors=False)
        if cover is not None:
            if isinstance(cover, str) and os.path.exists(cover):
                with open(cover, 'rb') as f:
                    cover = f.read()
            self.set_cover({book_id: cover})
        self._fire('books_added', (book_id,))
        return book_id

    def add_books(
        self, books, add_duplicates=True, apply_import_tags=True, preserve_uuid=False, run_hooks=True, dbapi=None
    ):
        """`books` is a list of (Metadata, {FMT: path}); returns (ids added,
        (Metadata, format map) pairs skipped as duplicates)."""
        duplicates, ids = [], []
        fmt_map = {}
        for mi, format_map in books:
            book_id = self.create_book_entry(
                mi, add_duplicates=add_duplicates, apply_import_tags=apply_import_tags, preserve_uuid=preserve_uuid
            )
            if book_id is None:
                duplicates.append((mi, format_map))
            else:
                fmt_map[book_id] = format_map
                ids.append(book_id)
                for fmt, stream_or_path in format_map.items():
                    if self.add_format(book_id, fmt, stream_or_path, dbapi=dbapi, run_hooks=run_hooks):
                        pass
        if run_hooks and ids:
            from calibre.customize.ui import run_plugins_on_postadd

            for book_id in ids:
                run_plugins_on_postadd(dbapi or self, book_id, fmt_map[book_id])
        return ids, duplicates

    def remove_books(self, book_ids, permanent=False):
        """Delete books (their files too)."""
        book_ids = [b for b in book_ids if self.has_id(b)]
        paths = {book_id: self.field_for('path', book_id) for book_id in book_ids}
        with self.conn:
            for book_id in book_ids:
                for lt in (
                    'books_authors_link',
                    'books_tags_link',
                    'books_languages_link',
                    'books_series_link',
                    'books_publishers_link',
                    'books_ratings_link',
                    'identifiers',
                    'comments',
                    'data',
                    'books_plugin_data',
                    'metadata_dirtied',
                    'conversion_options',
                ):
                    self.conn.execute(f'DELETE FROM {lt} WHERE book=?', (book_id,))
                for data in self.backend.custom_column_label_map.values():
                    table, lt = self.backend.custom_table_names(data['num'])
                    if data['datatype'] == 'composite':
                        continue
                    if data['normalized']:
                        self.conn.execute(f'DELETE FROM {lt} WHERE book=?', (book_id,))
                    else:
                        self.conn.execute(f'DELETE FROM {table} WHERE book=?', (book_id,))
                self.conn.execute('DELETE FROM books WHERE id=?', (book_id,))
        self.backend.remove_books(paths, permanent=permanent)
        self._fire('books_removed', tuple(book_ids))

    def delete_trash_entry(self, *a, **k):
        pass

    def list_trash_entries(self):
        return [], []

    def expire_old_trash(self, *a, **k):
        pass

    # -- Searching and sorting --

    def search(self, query, restriction='', virtual_fields=None, book_ids=None, use_virtual_library=True):
        """The ids matching a calibre search, as a set."""
        from calibre.db.search import Search

        if self._search is None:
            self._search = Search(self, 'saved_searches', self.field_metadata.get_search_terms())
        return self._search(self, query, restriction, virtual_fields=virtual_fields, book_ids=book_ids)

    def books_in_virtual_library(self, vl, search_restriction=None, virtual_fields=None):
        vls = self.backend.prefs.get('virtual_libraries', {}) or {}
        q = vls.get(vl, '') if vl else ''
        if not q:
            return self.all_book_ids()
        return self.search(q, search_restriction or '', virtual_fields=virtual_fields)

    def number_of_books_in_virtual_library(self, vl=None, search_restriction=None):
        return len(self.books_in_virtual_library(vl, search_restriction))

    def virtual_libraries_for_books(self, book_ids, virtual_fields=None):
        return {b: () for b in book_ids}

    def multisort(self, fields, ids_to_sort=None, virtual_fields=None):
        """Sort book ids by [(field, ascending), …]."""
        ids = list(self.all_book_ids() if ids_to_sort is None else ids_to_sort)
        for field, ascending in reversed(list(fields)):
            field = 'sort' if field == 'title' else field

            def key(book_id, field=field):
                v = self.field_for(field, book_id)
                if v is None:
                    return (0, b'')
                if isinstance(v, (int, float)):
                    return (1, v)
                if hasattr(v, 'timestamp'):
                    return (1, v.timestamp())
                if isinstance(v, (tuple, list)):
                    v = ','.join(str(x) for x in v)
                if isinstance(v, dict):
                    v = ','.join(f'{k}:{x}' for k, x in v.items())
                return (2, sort_key(str(v)))

            ids.sort(key=key, reverse=not ascending)
        return ids

    # -- Identical books --

    def has_book(self, mi):
        title = getattr(mi, 'title', None)
        if not title:
            return False
        return self.conn.execute('SELECT 1 FROM books WHERE title=? COLLATE NOCASE', (title,)).fetchone() is not None

    def data_for_find_identical_books(self):
        from calibre.db.utils import fuzzy_title

        return (
            {r[0]: r[1] for r in self.conn.execute('SELECT id, name FROM authors')},
            {b: fuzzy_title(t) for b, t in self.conn.execute('SELECT id, title FROM books')},
        )

    def find_identical_books(self, mi, search_restriction='', book_ids=None):
        from calibre.db.utils import fuzzy_title

        if not mi.title:
            return set()
        ft = fuzzy_title(mi.title)
        candidates = set()
        for book_id, title in self.conn.execute('SELECT id, title FROM books'):
            if fuzzy_title(title) == ft:
                candidates.add(book_id)
        if book_ids is not None:
            candidates &= set(book_ids)
        if search_restriction:
            candidates &= self.search(search_restriction)
        if not candidates:
            return set()
        authors = {icu_lower(a) for a in (mi.authors or []) if a and a != 'Unknown'}
        if not authors:
            return candidates
        ans = set()
        for book_id in candidates:
            book_authors = {icu_lower(a) for a in self.field_for('authors', book_id, ())}
            if authors & book_authors:
                ans.add(book_id)
        return ans

    def books_with_same_title(self, mi, all_matches=True):
        return {b for (b,) in self.conn.execute('SELECT id FROM books WHERE title=? COLLATE NOCASE', (mi.title or '',))}

    # -- Items --

    def rename_items(self, field, item_id_to_new_name_map, change_index=True, restrict_to_book_ids=None):
        t = self._item_table(field)
        if t is None:
            return set(), {}
        affected = set()
        with self.conn:
            for item_id, new_name in item_id_to_new_name_map.items():
                row = self.conn.execute(
                    f'SELECT id FROM {t[0]} WHERE {t[1]}=? COLLATE NOCASE AND id != ?', (new_name, item_id)
                ).fetchone()
                books = self.books_for_field(field, item_id)
                if row is not None:
                    # Merge into the existing item.
                    self.conn.execute(f'UPDATE OR IGNORE {t[2]} SET {t[3]}=? WHERE {t[3]}=?', (row[0], item_id))
                    self.conn.execute(f'DELETE FROM {t[2]} WHERE {t[3]}=?', (item_id,))
                    self.conn.execute(f'DELETE FROM {t[0]} WHERE id=?', (item_id,))
                else:
                    self.conn.execute(f'UPDATE {t[0]} SET {t[1]}=? WHERE id=?', (new_name, item_id))
                    if field == 'authors':
                        self.conn.execute(
                            'UPDATE authors SET sort=? WHERE id=?', (author_to_author_sort(new_name), item_id)
                        )
                affected |= books
            self._touch(affected)
            for book_id in affected:
                if field == 'authors':
                    self.conn.execute(
                        'UPDATE books SET author_sort=? WHERE id=?',
                        (self.author_sort_from_authors(self.field_for('authors', book_id, ())), book_id),
                    )
        return affected, {}

    def remove_items(self, field, item_ids, restrict_to_book_ids=None):
        t = self._item_table(field)
        if t is None:
            return set()
        affected = set()
        with self.conn:
            for item_id in item_ids:
                books = self.books_for_field(field, item_id)
                if restrict_to_book_ids is not None:
                    books &= set(restrict_to_book_ids)
                    for b in books:
                        self.conn.execute(f'DELETE FROM {t[2]} WHERE {t[3]}=? AND book=?', (item_id, b))
                else:
                    self.conn.execute(f'DELETE FROM {t[2]} WHERE {t[3]}=?', (item_id,))
                    self.conn.execute(f'DELETE FROM {t[0]} WHERE id=?', (item_id,))
                affected |= books
            self._touch(affected)
        return affected

    def set_sort_for_authors(self, author_id_to_sort_map, update_books=True):
        changed = set()
        with self.conn:
            for aid, sort in author_id_to_sort_map.items():
                self.conn.execute('UPDATE authors SET sort=? WHERE id=?', (sort, aid))
                if update_books:
                    for b in self.books_for_field('authors', aid):
                        self.conn.execute(
                            'UPDATE books SET author_sort=? WHERE id=?',
                            (self.author_sort_from_authors(self.field_for('authors', b, ())), b),
                        )
                        changed.add(b)
            self._touch(changed)
        return changed

    def set_link_for_authors(self, author_id_to_link_map):
        with self.conn:
            for aid, link in author_id_to_link_map.items():
                self.conn.execute('UPDATE authors SET link=? WHERE id=?', (link or '', aid))
        return set()

    def tags_older_than(self, tag, delta=None, must_have_tag=None, must_have_authors=None):
        from datetime import timedelta

        tag_id = self.get_item_id('tags', tag)
        if tag_id is None:
            return set()
        books = self.books_for_field('tags', tag_id)
        if must_have_tag:
            mid = self.get_item_id('tags', must_have_tag)
            books &= self.books_for_field('tags', mid) if mid else set()
        if must_have_authors:
            wanted = {icu_lower(a) for a in must_have_authors}
            books = {b for b in books if {icu_lower(a) for a in self.field_for('authors', b, ())} & wanted}
        if delta is None:
            return books
        if not isinstance(delta, timedelta):
            delta = timedelta(days=delta)
        now = utcnow()
        return {b for b in books if (now - self.field_for('timestamp', b, UNDEFINED_DATE)) > delta}

    # -- Custom columns --

    def create_custom_column(self, label, name, datatype, is_multiple, editable=True, display={}):
        num = self.backend.create_custom_column(label, name, datatype, is_multiple, editable=editable, display=display)
        self.field_metadata = self.backend.field_metadata
        return num

    def delete_custom_column(self, label=None, num=None):
        self.backend.delete_custom_column(label, num)

    def set_custom_column_metadata(
        self, num, name=None, label=None, is_editable=None, display=None, update_last_modified=False
    ):
        return self.backend.set_custom_column_metadata(
            num, name=name, label=label, is_editable=is_editable, display=display
        )

    def custom_field_label_map(self):
        return self.backend.custom_column_label_map

    # -- Plugin data (books_plugin_data) --

    def add_custom_book_data(self, name, val_map, delete_first=False):
        self.backend.add_custom_data(name, val_map, delete_first)

    def get_custom_book_data(self, name, book_ids=(), default=None):
        return self.backend.get_custom_book_data(name, book_ids, default)

    def delete_custom_book_data(self, name, book_ids=()):
        self.backend.delete_custom_book_data(name, book_ids)

    def get_ids_for_custom_book_data(self, name):
        return self.backend.get_ids_for_custom_book_data(name)

    # -- Misc --

    def dirtied(self, book_ids):
        self.backend.dirty_books(book_ids)

    def dirty_queue_length(self):
        return 0

    def mark_as_dirty(self, book_ids):
        self.dirtied(book_ids)

    def conversion_options(self, book_id, fmt='PIPE'):
        return self.backend.conversion_options(book_id, fmt)

    def has_conversion_options(self, ids, fmt='PIPE'):
        return self.backend.has_conversion_options(ids, fmt)

    def delete_conversion_options(self, book_ids, fmt='PIPE'):
        self.backend.delete_conversion_options(book_ids, fmt)

    def set_conversion_options(self, options, fmt='PIPE'):
        self.backend.set_conversion_options(options, fmt)

    def get_id_map(self, field):
        t = self._item_table(field)
        if t is None:
            return {}
        return {r[0]: r[1] for r in self.conn.execute(f'SELECT id, {t[1]} FROM {t[0]}')}

    def get_item_ids_for_names(self, field, names):
        return self.get_item_ids(field, names)

    def all_field_keys(self):
        return self.field_metadata.all_field_keys()

    def read_backup(self, book_id):
        return None

    def dump_metadata(self, book_ids=None, remove_from_dirtied=True, callback=None):
        pass

    def reindex_fts(self):
        pass

    def is_fts_enabled(self):
        return False

    def vacuum(self, *a, **k):
        self.backend.vacuum()

    def clear_search_caches(self, book_ids=None):
        pass

    def get_next_series_num_for(self, series, field='series', current_indices=False):
        sid = self.get_item_id(field, series)
        if sid is None:
            return 1.0
        books = self.books_for_field(field, sid)
        idx_field = 'series_index' if field == 'series' else field + '_index'
        indices = sorted(self.field_for(idx_field, b, 1.0) or 1.0 for b in books)
        if current_indices:
            return {b: self.field_for(idx_field, b, 1.0) for b in books}
        return _get_next_series_num_for_list(tuple(indices), unwrap=False)

    def split_if_is_multiple_composite(self, f, val):
        return val

    def format_sizes(self, book_id):
        return {
            f.upper(): s
            for f, s in self.conn.execute('SELECT format, uncompressed_size FROM data WHERE book=?', (book_id,))
        }

    def new_api_for_db(self):
        return self


__all__ = ['Cache', 'api', 'read_api', 'write_api', 'wraps', 'BytesIO', 'authors_to_string', 'DB']
