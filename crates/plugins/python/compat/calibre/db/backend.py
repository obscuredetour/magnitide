# Derived from calibre's src/calibre/db/backend.py (calibre 9.15).
# Copyright: 2011, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.backend` (calibre 9.15) for Magnitide: the
SQLite layer under `Cache`, on the shadow library's `metadata.db`.

calibre's schema has triggers and views that call functions calibre
registers on its connection (`title_sort`, `uuid4`, `concat`,
`sortconcat`, `books_list_filter`, `author_to_author_sort`); they are
registered here too, so plugins' raw SQL (`db.conn`) works. Dates are
stored as calibre does (ISO text, UTC). Every change to a book stamps its
`last_modified`, which is how Magnitide finds what a plugin changed.
"""

import json
import os
import re
import shutil
import sqlite3
import time
from contextlib import suppress
from functools import partial

from calibre import isbytestring, prints, sanitize_file_name
from calibre.constants import filesystem_encoding, iswindows
from calibre.db.constants import COVER_FILE_NAME
from calibre.ebooks.metadata import author_to_author_sort, title_sort
from calibre.library.field_metadata import FieldMetadata
from calibre.utils.config import from_json, prefs, to_json, tweaks
from calibre.utils.date import UNDEFINED_DATE, isoformat, parse_date, utcnow
from calibre.utils.filenames import ascii_filename
from calibre.utils.icu import sort_key

CUSTOM_DATA_TYPES = frozenset(
    ['rating', 'text', 'comments', 'datetime', 'int', 'float', 'bool', 'series', 'composite', 'enumeration']
)
WINDOWS_RESERVED_NAMES = frozenset(
    'CON PRN AUX NUL COM1 COM2 COM3 COM4 COM5 COM6 COM7 COM8 COM9 LPT1 LPT2 LPT3 LPT4 LPT5 LPT6 LPT7 LPT8 LPT9'.split()
)


# --- SQL functions calibre registers ---------------------------------------


class _Concat:
    def __init__(self):
        self.ans = []

    def step(self, value):
        if value is not None:
            self.ans.append(value)

    def finalize(self):
        return ','.join(str(x) for x in self.ans) if self.ans else None


class _SortConcat:
    def __init__(self):
        self.ans = []

    def step(self, ndx, value):
        if value is not None:
            self.ans.append((ndx, value))

    def finalize(self):
        self.ans.sort(key=lambda x: x[0])
        return ','.join(str(x[1]) for x in self.ans) if self.ans else None


class _SortConcatBar(_SortConcat):
    def finalize(self):
        self.ans.sort(key=lambda x: x[0])
        return '|'.join(str(x[1]) for x in self.ans) if self.ans else None


class _IdentifiersConcat:
    def __init__(self):
        self.ans = []

    def step(self, key, val):
        self.ans.append('%s:%s' % (key, val))

    def finalize(self):
        return ','.join(self.ans) if self.ans else None


class _AumSortedConcatenate:
    def __init__(self):
        self.ans = {}

    def step(self, ndx, author, sort, link):
        if author is not None:
            self.ans[ndx] = ':::'.join((author, sort or '', link or ''))

    def finalize(self):
        keys = sorted(self.ans)
        return ':#:'.join(self.ans[k] for k in keys) if keys else None


def register_functions(conn):
    conn.create_function('title_sort', 1, lambda t: title_sort(t) if t else t, deterministic=True)
    conn.create_function('author_to_author_sort', 1, lambda a: author_to_author_sort(a) if a else a, deterministic=True)
    conn.create_function('uuid4', 0, lambda: __import__('uuid').uuid4().__str__())
    conn.create_function('books_list_filter', 1, lambda book: 1)
    conn.create_function('lower', 1, lambda x: x.lower() if isinstance(x, str) else x, deterministic=True)
    conn.create_aggregate('concat', 1, _Concat)
    conn.create_aggregate('sortconcat', 2, _SortConcat)
    conn.create_aggregate('sortconcat_bar', 2, _SortConcatBar)
    conn.create_aggregate('identifiers_concat', 2, _IdentifiersConcat)
    conn.create_aggregate('aum_sortconcat', 4, _AumSortedConcatenate)
    conn.create_collation('icucollate', lambda a, b: (sort_key(a) > sort_key(b)) - (sort_key(a) < sort_key(b)))


# --- Dates -----------------------------------------------------------------


def dt_to_db(dt):
    if dt is None:
        return None
    if isinstance(dt, str):
        return dt
    from calibre.utils.date import as_utc

    return isoformat(as_utc(dt), sep=' ')


def db_to_dt(raw):
    if raw is None or raw == '':
        return None
    try:
        return parse_date(raw, assume_utc=True)
    except Exception:
        return UNDEFINED_DATE


# --- Prefs ----------------------------------------------------------------


class DBPrefs(dict):
    """The `preferences` table as a dict; changes are written at once."""

    def __init__(self, db):
        dict.__init__(self)
        self.db = db
        self.defaults = {}
        self.disable_setting = False
        self.load_from_db()

    def load_from_db(self):
        self.clear()
        for key, val in self.db.conn.execute('SELECT key,val FROM preferences'):
            try:
                val = self.raw_to_object(val)
            except Exception:
                prints('Failed to read value for:', key, 'from db')
                continue
            dict.__setitem__(self, key, val)

    def raw_to_object(self, raw):
        if not isinstance(raw, str):
            raw = raw.decode('utf-8', 'replace')
        return json.loads(raw, object_hook=from_json)

    def to_raw(self, val):
        return json.dumps(val, indent=2, default=to_json)

    def has_setting(self, key):
        return key in self

    def __getitem__(self, key):
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return self.defaults[key]

    def __delitem__(self, key):
        dict.__delitem__(self, key)
        self.db.conn.execute('DELETE FROM preferences WHERE key=?', (key,))
        self.db.conn.commit()

    def __setitem__(self, key, val):
        if not self.disable_setting:
            raw = self.to_raw(val)
            with self.db.conn:
                self.db.conn.execute('INSERT OR REPLACE INTO preferences (key,val) VALUES (?,?)', (key, raw))
            dict.__setitem__(self, key, val)

    def set(self, key, val):
        self.__setitem__(key, val)

    def get_namespaced(self, namespace, key, default=None):
        key = 'namespaced:%s:%s' % (namespace, key)
        try:
            return dict.__getitem__(self, key)
        except KeyError:
            return default

    def set_namespaced(self, namespace, key, val):
        if ':' in key:
            raise KeyError('Colons are not allowed in keys')
        if ':' in namespace:
            raise KeyError('Colons are not allowed in the namespace')
        key = 'namespaced:%s:%s' % (namespace, key)
        self[key] = val

    def write_serialized(self, library_path):
        pass


# --- The backend -----------------------------------------------------------


class DB:
    """The database. `conn` is the sqlite3 connection (plugins' raw SQL
    goes there). Book files live under `library_path/<path>/`."""

    PATH_LIMIT = 40 if iswindows else 100
    WINDOWS_LIBRARY_PATH_LIMIT = 75

    def __init__(
        self,
        library_path,
        default_prefs=None,
        read_only=False,
        restore_all_prefs=False,
        progress_callback=lambda x, y: True,
        load_user_formatter_functions=True,
        temp_db=False,
    ):
        self.library_path = os.path.abspath(library_path)
        self.dbpath = os.path.join(self.library_path, 'metadata.db')
        if not os.path.exists(self.dbpath):
            raise ValueError(f'No calibre library at {self.library_path!r} (no metadata.db)')
        self.is_closed = False
        self.read_only = read_only
        self._conn = None
        self.conn  # noqa: B018 (open it)
        self.prefs = DBPrefs(self)
        self.prefs.defaults['gui_restriction'] = ''
        self.prefs.defaults['virtual_lib_on_startup'] = ''
        self.prefs.defaults['virtual_libraries'] = {}
        self.prefs.defaults['virtual_lib_on_startup'] = ''
        self.prefs.defaults['saved_searches'] = {}
        self.prefs.defaults['user_categories'] = {}
        self.prefs.defaults['grouped_search_terms'] = {}
        self.prefs.defaults['similar_authors_search_key'] = 'authors'
        self.prefs.defaults['similar_authors_match_kind'] = 'match_any'
        self.prefs.defaults['similar_publisher_search_key'] = 'publisher'
        self.prefs.defaults['similar_publisher_match_kind'] = 'match_any'
        self.prefs.defaults['similar_tags_search_key'] = 'tags'
        self.prefs.defaults['similar_tags_match_kind'] = 'match_all'
        self.prefs.defaults['similar_series_search_key'] = 'series'
        self.prefs.defaults['similar_series_match_kind'] = 'match_any'
        self.prefs.defaults['book_display_fields'] = []
        self.prefs.defaults['update_all_last_mod_dates_on_start'] = False
        self.prefs.defaults['field_under_covers_in_grid'] = 'title'
        self.prefs.defaults['cover_browser_title_template'] = '{title}'
        self.prefs.defaults['cover_browser_subtitle_field'] = 'rating'
        self.prefs.defaults['styled_columns'] = {}
        self.prefs.defaults['edit_metadata_ignore_display_order'] = False
        self.prefs.defaults['fts_enabled'] = False
        self.prefs.defaults['tag_browser_category_order'] = {}
        self.prefs.defaults['last_expired_trash_scan'] = 0
        self.prefs.defaults['expire_old_trash_after'] = 14 * 86400
        self.prefs.defaults['books_in_virtual_library'] = ''
        self.prefs.defaults['column_color_rules'] = []
        self.prefs.defaults['column_icon_rules'] = []
        self.prefs.defaults['cover_grid_icon_rules'] = []
        self.prefs.defaults['grouped_search_make_user_categories'] = []
        self.prefs.defaults['categories_using_hierarchy'] = []
        self.prefs.defaults['bools_are_tristate'] = True
        self.field_metadata = FieldMetadata()
        self.custom_column_label_map, self.custom_column_num_map = {}, {}
        self._library_id = None
        self.initialize_custom_columns()
        self.initialize_prefs(default_prefs, restore_all_prefs, progress_callback)

    @property
    def conn(self):
        if self._conn is None:
            self._conn = sqlite3.connect(self.dbpath, check_same_thread=False, timeout=30)
            self._conn.row_factory = None
            register_functions(self._conn)
            self._conn.execute('PRAGMA foreign_keys=OFF')
            self._conn.execute('PRAGMA journal_mode=WAL')
            self._conn.execute('PRAGMA synchronous=NORMAL')
        return self._conn

    def execute(self, sql, bindings=None):
        if bindings is None:
            return self.conn.execute(sql)
        return self.conn.execute(sql, bindings)

    def executemany(self, sql, sequence):
        return self.conn.executemany(sql, sequence)

    def get(self, *args, **kw):
        ans = self.conn.execute(*args)
        if kw.get('all', True):
            return ans.fetchall()
        row = ans.fetchone()
        return row[0] if row else None

    def last_insert_rowid(self):
        return self.conn.execute('SELECT last_insert_rowid()').fetchone()[0]

    def commit(self):
        with suppress(Exception):
            self.conn.commit()

    def close(self, force=False, unload_formatter_functions=True):
        if self._conn is not None:
            with suppress(Exception):
                self._conn.commit()
            with suppress(Exception):
                self._conn.close()
            self._conn = None
        self.is_closed = True

    def reopen(self, force=False):
        self.close(force)
        self.is_closed = False
        self.conn  # noqa: B018

    def vacuum(self, include_fts_db=False, include_notes_db=False):
        self.conn.execute('VACUUM')

    @property
    def library_id(self):
        if self._library_id is None:
            ans = self.get('SELECT uuid FROM library_id', all=False)
            if ans is None:
                import uuid

                ans = str(uuid.uuid4())
                with self.conn:
                    self.conn.execute('INSERT INTO library_id (uuid) VALUES (?)', (ans,))
            self._library_id = ans
        return self._library_id

    @library_id.setter
    def library_id(self, val):
        self._library_id = str(val)
        with self.conn:
            self.conn.execute('DELETE FROM library_id')
            self.conn.execute('INSERT INTO library_id (uuid) VALUES (?)', (self._library_id,))

    def initialize_prefs(self, default_prefs, restore_all_prefs, progress_callback):
        if default_prefs is not None:
            for key in default_prefs:
                if key not in self.prefs or restore_all_prefs:
                    self.prefs[key] = default_prefs[key]
        from calibre.library.field_metadata import fm_as_dict  # noqa: F401

        self.prefs['field_metadata'] = self.field_metadata.all_metadata()

    # -- Custom columns --

    def initialize_custom_columns(self):
        self.custom_column_label_map, self.custom_column_num_map = {}, {}
        self.custom_column_label_map = {}
        rows = self.conn.execute(
            'SELECT id, label, name, datatype, mark_for_delete, editable, display, is_multiple, normalized FROM custom_columns'
        ).fetchall()
        for row in rows:
            num, label, name, datatype, mark_for_delete, editable, display, is_multiple, normalized = row
            if mark_for_delete:
                continue
            try:
                display = json.loads(display or '{}', object_hook=from_json)
            except Exception:
                display = {}
            data = {
                'num': num,
                'label': label,
                'name': name,
                'datatype': datatype,
                'editable': bool(editable),
                'display': display,
                'normalized': bool(normalized),
                'is_multiple': bool(is_multiple),
                'is_editable': bool(editable),
            }
            if datatype == 'composite':
                data['normalized'] = False
            self.custom_column_label_map[label] = data
            self.custom_column_num_map[num] = data
            table, lt = self.custom_table_names(num)
            if data['normalized']:
                data['table'] = table
                data['link_table'] = lt
            else:
                data['table'] = table
                data['link_table'] = None
            if datatype == 'text' and is_multiple:
                im = {'cache_to_list': '|', 'ui_to_list': ',', 'list_to_ui': ', '}
            elif datatype == 'composite' and is_multiple:
                im = {'cache_to_list': ',', 'ui_to_list': ',', 'list_to_ui': ', '}
            else:
                im = {}
            data['multiple_seps'] = im
            key = '#' + label
            if key not in self.field_metadata:
                self.field_metadata.add_custom_field(
                    label=label,
                    table=table,
                    column='value',
                    datatype=datatype,
                    colnum=num,
                    name=name,
                    display=display,
                    is_editable=bool(editable),
                    is_multiple=im,
                    is_category=datatype in ('text', 'series', 'enumeration', 'rating', 'bool')
                    and not (datatype == 'text' and False),
                )
            self.field_metadata[key]['rec_index'] = num
            self.field_metadata[key]['normalized'] = data['normalized']
            self.field_metadata[key]['link_column'] = 'value'

    @staticmethod
    def custom_table_names(num):
        return 'custom_column_%d' % num, 'books_custom_column_%d_link' % num

    def custom_field_name(self, label=None, num=None):
        if label is not None:
            return self.field_metadata.custom_field_prefix + label
        return self.field_metadata.custom_field_prefix + self.custom_column_num_map[num]['label']

    def custom_field_metadata(self, label=None, num=None):
        if label is not None:
            return self.custom_column_label_map[label]
        return self.custom_column_num_map[num]

    def create_custom_column(self, label, name, datatype, is_multiple, editable=True, display={}):
        import re

        if not label:
            raise ValueError('No label was provided')
        if re.match(r'^\w*$', label) is None or not label[0].isalpha() or label.lower() != label:
            raise ValueError(
                'The label must contain only lower case letters, digits and underscores, and start with a letter'
            )
        if datatype not in CUSTOM_DATA_TYPES:
            raise ValueError('%r is not a supported data type' % datatype)
        normalized = datatype not in ('datetime', 'comments', 'int', 'bool', 'float', 'composite')
        is_multiple = is_multiple and datatype in ('text', 'composite')
        with self.conn:
            self.conn.execute(
                'INSERT INTO custom_columns (label,name,datatype,is_multiple,editable,display,normalized) VALUES (?,?,?,?,?,?,?)',
                (label, name, datatype, is_multiple, editable, json.dumps(display, default=to_json), normalized),
            )
            num = self.last_insert_rowid()
            if datatype in ('rating', 'int'):
                dt = 'INT'
            elif datatype in ('text', 'comments', 'series', 'composite', 'enumeration'):
                dt = 'TEXT'
            elif datatype in ('float',):
                dt = 'REAL'
            elif datatype == 'datetime':
                dt = 'timestamp'
            elif datatype == 'bool':
                dt = 'BOOL'
            collate = 'COLLATE NOCASE' if dt == 'TEXT' else ''
            table, lt = self.custom_table_names(num)
            if normalized:
                if datatype == 'series':
                    s_index = 'extra REAL,'
                else:
                    s_index = ''
                lines = [
                    'CREATE TABLE %s(id INTEGER PRIMARY KEY AUTOINCREMENT, value %s NOT NULL %s, link TEXT NOT NULL DEFAULT "", UNIQUE(value));'
                    % (table, dt, collate),
                    'CREATE INDEX %s_idx ON %s (value %s);' % (table, table, collate),
                    'CREATE TABLE %s(id INTEGER PRIMARY KEY AUTOINCREMENT, book INTEGER NOT NULL, value INTEGER NOT NULL, %s UNIQUE(book, value));'
                    % (lt, s_index),
                    'CREATE INDEX %s_aidx ON %s (value);' % (lt, lt),
                    'CREATE INDEX %s_bidx ON %s (book);' % (lt, lt),
                    'CREATE TRIGGER fkc_delete_%s AFTER DELETE ON %s BEGIN DELETE FROM %s WHERE value=OLD.id; END;'
                    % (lt, table, lt),
                ]
            else:
                lines = [
                    'CREATE TABLE %s(id INTEGER PRIMARY KEY AUTOINCREMENT, book INTEGER, value %s NOT NULL %s, UNIQUE(book));'
                    % (table, dt, collate),
                    'CREATE INDEX %s_idx ON %s (book);' % (table, table),
                ]
            self.conn.executescript('\n'.join(lines))
        self.initialize_custom_columns()
        return num

    def delete_custom_column(self, label=None, num=None):
        data = self.custom_field_metadata(label, num)
        with self.conn:
            self.conn.execute('UPDATE custom_columns SET mark_for_delete=1 WHERE id=?', (data['num'],))

    def set_custom_column_metadata(
        self, num, name=None, label=None, is_editable=None, display=None, notify=True, update_last_modified=False
    ):
        changed = False
        with self.conn:
            if name is not None:
                self.conn.execute('UPDATE custom_columns SET name=? WHERE id=?', (name, num))
                changed = True
            if label is not None:
                self.conn.execute('UPDATE custom_columns SET label=? WHERE id=?', (label, num))
                changed = True
            if is_editable is not None:
                self.conn.execute('UPDATE custom_columns SET editable=? WHERE id=?', (bool(is_editable), num))
                changed = True
            if display is not None:
                self.conn.execute(
                    'UPDATE custom_columns SET display=? WHERE id=?', (json.dumps(display, default=to_json), num)
                )
                changed = True
        if changed:
            # The field's description may have changed shape: rebuild.
            self.field_metadata.remove_custom_fields()
            self.initialize_custom_columns()
        return changed

    # -- Files --

    def construct_path_name(self, book_id, title, author):
        """calibre's folder for a book: 'Author/Title (id)'."""
        book_id = ' (%d)' % book_id
        l = self.PATH_LIMIT - (len(book_id) // 2) - 2
        author = ascii_filename(author)[:l]
        title = ascii_filename(title.lstrip())[:l].rstrip()
        if not title:
            title = 'Unknown'[:l]
        try:
            while author[-1] in (' ', '.'):
                author = author[:-1]
        except IndexError:
            author = ''
        if not author:
            author = ascii_filename('Unknown')
        if author.upper() in WINDOWS_RESERVED_NAMES:
            author += 'w'
        return '%s/%s%s' % (author, title, book_id)

    def construct_file_name(self, book_id, title, author, extlen):
        """calibre's file name for a book's formats: 'Title - Author'."""
        extlen = max(extlen, 14)
        l = self.PATH_LIMIT - (extlen // 2) - 2
        author = ascii_filename(author)[:l]
        title = ascii_filename(title.lstrip())[:l].rstrip()
        if not title:
            title = 'Unknown'[:l]
        name = title + ' - ' + author
        while name.endswith('.'):
            name = name[:-1]
        if not name:
            name = ascii_filename('Unknown')
        return name

    def format_abspath(self, book_id, fmt, fname, path):
        path = os.path.join(self.library_path, path)
        fmt = ('.' + fmt.lower()) if fmt else ''
        fmt_path = os.path.join(path, fname + fmt)
        if os.path.exists(fmt_path):
            return fmt_path
        try:
            candidates = os.listdir(path)
        except OSError:
            return None
        if fmt and candidates:
            for x in candidates:
                if x.lower().endswith(fmt):
                    return os.path.join(path, x)
        return None

    def cover_abspath(self, book_id, path):
        path = os.path.join(self.library_path, path)
        fmt_path = os.path.join(path, COVER_FILE_NAME)
        if os.path.exists(fmt_path):
            return fmt_path
        return None

    def apply_to_format(self, book_id, path, fname, fmt, func, missing_value=None):
        path = self.format_abspath(book_id, fmt, fname, path)
        if path is None:
            return missing_value
        with open(path, 'r+b') as f:
            return func(f)

    def format_hash(self, book_id, fmt, fname, path):
        import hashlib

        path = self.format_abspath(book_id, fmt, fname, path)
        if path is None:
            from calibre.db.errors import NoSuchFormat

            raise NoSuchFormat('Record %d has no fmt: %s' % (book_id, fmt))
        sha = hashlib.sha256()
        with open(path, 'rb') as f:
            while True:
                raw = f.read(1 << 20)
                if not raw:
                    break
                sha.update(raw)
        return sha.hexdigest()

    def format_metadata(self, book_id, fmt, fname, path):
        path = self.format_abspath(book_id, fmt, fname, path)
        ans = {}
        if path is not None:
            stat = os.stat(path)
            ans['path'] = path
            ans['size'] = stat.st_size
            from calibre.utils.date import utcfromtimestamp

            ans['mtime'] = utcfromtimestamp(stat.st_mtime)
        return ans

    def has_format(self, book_id, fmt, fname, path):
        return self.format_abspath(book_id, fmt, fname, path) is not None

    def copy_cover_to(self, path, dest, use_hardlink=False, report_file_size=None):
        path = os.path.abspath(os.path.join(self.library_path, path, COVER_FILE_NAME))
        if os.path.exists(path):
            if hasattr(dest, 'write'):
                with open(path, 'rb') as f:
                    shutil.copyfileobj(f, dest)
                if hasattr(dest, 'flush'):
                    dest.flush()
                return True
            if dest and not os.path.samefile(dest, path) if os.path.exists(dest) else dest:
                shutil.copyfile(path, dest)
                return True
        return False

    def cover_last_modified(self, path):
        path = os.path.abspath(os.path.join(self.library_path, path, COVER_FILE_NAME))
        try:
            from calibre.utils.date import utcfromtimestamp

            return utcfromtimestamp(os.stat(path).st_mtime)
        except OSError:
            pass

    def copy_format_to(self, book_id, fmt, fname, path, dest, use_hardlink=False, report_file_size=None):
        path = self.format_abspath(book_id, fmt, fname, path)
        if path is None:
            return False
        if hasattr(dest, 'write'):
            with open(path, 'rb') as f:
                shutil.copyfileobj(f, dest)
            if hasattr(dest, 'flush'):
                dest.flush()
        elif dest and (not os.path.exists(dest) or not os.path.samefile(dest, path)):
            shutil.copyfile(path, dest)
        return True

    def set_cover(self, book_id, path, data, no_processing=False):
        path = os.path.abspath(os.path.join(self.library_path, path))
        if not os.path.exists(path):
            os.makedirs(path)
        path = os.path.join(path, COVER_FILE_NAME)
        if callable(getattr(data, 'save', None)):
            from calibre.gui2 import pixmap_to_data

            data = pixmap_to_data(data)
        elif callable(getattr(data, 'read', None)):
            data = data.read()
        if data is None:
            if os.path.lexists(path):
                os.remove(path)
        else:
            if not no_processing:
                try:
                    from calibre.utils.img import save_cover_data_to

                    save_cover_data_to(data, path)
                    return
                except Exception:
                    pass
            if os.path.islink(path):
                os.remove(path)
            with open(path, 'wb') as f:
                f.write(data)

    def add_format(self, book_id, fmt, stream, title, author, path, current_name, mtime=None):
        fmt = ('.' + fmt.lower()) if fmt else ''
        fname = self.construct_file_name(book_id, title, author, len(fmt))
        path = os.path.join(self.library_path, path)
        dest = os.path.join(path, fname + fmt)
        if not os.path.exists(path):
            os.makedirs(path)
        size = 0
        if current_name is not None:
            old_path = os.path.join(path, current_name + fmt)
            if old_path != dest:
                # Ensure that the old format file is not orphaned
                if os.path.exists(old_path):
                    os.remove(old_path)
        if (
            not getattr(stream, 'name', False)
            or not os.path.exists(dest)
            or not os.path.samefile(getattr(stream, 'name', ''), dest)
        ):
            if os.path.islink(dest):
                os.remove(dest)
            with open(dest, 'wb') as f:
                shutil.copyfileobj(stream, f)
            size = os.path.getsize(dest)
        else:
            size = os.path.getsize(dest)
        if mtime is not None:
            os.utime(dest, (mtime, mtime))
        return size, fname

    def remove_formats(self, remove_map):
        """{book_id: {fmt: (fname, path)}}; files go to the trash folder."""
        paths = []
        for book_id, removals in remove_map.items():
            for fmt, fname, path in removals:
                p = self.format_abspath(book_id, fmt, fname, path)
                if p is not None:
                    paths.append(p)
        for p in paths:
            with suppress(OSError):
                os.remove(p)

    def remove_books(self, path_map, permanent=False):
        for book_id, path in path_map.items():
            if path:
                path = os.path.abspath(os.path.join(self.library_path, path))
                if os.path.exists(path):
                    shutil.rmtree(path, ignore_errors=True)
                    parent = os.path.dirname(path)
                    with suppress(OSError):
                        os.rmdir(parent)

    def update_path(self, book_id, title, author, path_field, formats_field):
        """calibre renames book folders when title/author change; Magnitide
        does its own renaming, so the shadow keeps its paths."""
        return

    # -- Custom book data (books_plugin_data) --

    def add_custom_data(self, name, val_map, delete_first):
        if delete_first:
            self.conn.execute('DELETE FROM books_plugin_data WHERE name=?', (name,))
        self.conn.executemany(
            'INSERT OR REPLACE INTO books_plugin_data (book, name, val) VALUES (?, ?, ?)',
            [(book_id, name, json.dumps(val, default=to_json)) for book_id, val in val_map.items()],
        )
        self.conn.commit()

    def get_custom_book_data(self, name, book_ids, default=None):
        book_ids = frozenset(book_ids)

        def safe_load(val):
            try:
                return json.loads(val, object_hook=from_json)
            except Exception:
                return default

        if len(book_ids) == 1:
            bid = next(iter(book_ids))
            ans = {
                book_id: safe_load(val)
                for book_id, val in self.conn.execute(
                    'SELECT book, val FROM books_plugin_data WHERE book=? AND name=?', (bid, name)
                )
            }
            return ans or {bid: default}
        ans = {}
        for book_id, val in self.conn.execute('SELECT book, val FROM books_plugin_data WHERE name=?', (name,)):
            if not book_ids or book_id in book_ids:
                val = safe_load(val)
                ans[book_id] = val
        return ans

    def delete_custom_book_data(self, name, book_ids):
        if book_ids:
            self.conn.executemany(
                'DELETE FROM books_plugin_data WHERE book=? AND name=?', [(book_id, name) for book_id in book_ids]
            )
        else:
            self.conn.execute('DELETE FROM books_plugin_data WHERE name=?', (name,))
        self.conn.commit()

    def get_ids_for_custom_book_data(self, name):
        return frozenset(r[0] for r in self.conn.execute('SELECT book FROM books_plugin_data WHERE name=?', (name,)))

    # -- Misc --

    def dirty_books(self, book_ids):
        self.conn.executemany('INSERT OR IGNORE INTO metadata_dirtied (book) VALUES (?)', [(x,) for x in book_ids])

    def dirtied_books(self):
        return frozenset(r[0] for r in self.conn.execute('SELECT book FROM metadata_dirtied'))

    def mark_as_clean(self, book_ids):
        self.conn.executemany('DELETE FROM metadata_dirtied WHERE book=?', [(x,) for x in book_ids])

    def update_last_modified(self, book_ids, now=None):
        if now is None:
            now = utcnow()
        now = dt_to_db(now)
        self.conn.executemany('UPDATE books SET last_modified=? WHERE id=?', [(now, book_id) for book_id in book_ids])

    def conversion_options(self, book_id, fmt):
        for (data,) in self.conn.execute(
            'SELECT data FROM conversion_options WHERE book=? AND format=?', (book_id, fmt.upper())
        ):
            if data:
                from calibre.utils.serialize import pickle_loads

                return pickle_loads(bytes(data))

    def has_conversion_options(self, ids, fmt='PIPE'):
        ids = frozenset(ids)
        return any(
            r[0] in ids for r in self.conn.execute('SELECT book FROM conversion_options WHERE format=?', (fmt.upper(),))
        )

    def delete_conversion_options(self, book_ids, fmt):
        self.conn.executemany(
            'DELETE FROM conversion_options WHERE book=? AND format=?', [(book_id, fmt.upper()) for book_id in book_ids]
        )
        self.conn.commit()

    def set_conversion_options(self, options, fmt):
        from calibre.utils.serialize import pickle_dumps

        options = [(book_id, fmt.upper(), pickle_dumps(data)) for book_id, data in options.items()]
        self.conn.executemany('INSERT OR REPLACE INTO conversion_options(book,format,data) VALUES (?,?,?)', options)
        self.conn.commit()

    def get_books_for_category(self, category, item_id_or_composite_value):
        return frozenset()

    def all_book_ids(self):
        return frozenset(r[0] for r in self.conn.execute('SELECT id FROM books'))

    def backup_database(self, path):
        dest = sqlite3.connect(path)
        with dest:
            self.conn.backup(dest)
        dest.close()

    def check_integrity(self):
        pass

    def clear_fts(self):
        pass

    def initialize_fts(self, *a, **k):
        pass

    def enable_fts(self, *a, **k):
        raise NotImplementedError("Full-text search isn't available in Magnitide's plugin runtime")

    @property
    def fts_enabled(self):
        return False


__all__ = [
    'DB',
    'DBPrefs',
    'register_functions',
    'dt_to_db',
    'db_to_dt',
    'CUSTOM_DATA_TYPES',
    'partial',
    'prefs',
    'tweaks',
    'time',
    're',
    'isbytestring',
    'sanitize_file_name',
    'filesystem_encoding',
]
