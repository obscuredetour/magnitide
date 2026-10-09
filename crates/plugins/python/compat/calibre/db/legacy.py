# Derived from calibre's src/calibre/db/legacy.py (calibre 9.15).
# Copyright: 2013, Kovid Goyal <kovid at kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.legacy` (calibre 9.15): `LibraryDatabase`,
the old API most plugins use (`gui.current_db`), as a layer over `Cache`
(`db.new_api`).

Row methods take an index into `db.data` unless `index_is_id=True`, as in
calibre. List fields come back as strings (`tags` comma-joined, `formats`
comma-joined), as calibre's do.
"""

import os
import traceback
from contextlib import suppress

from calibre.db import _get_next_series_num_for_list, _get_series_values, get_data_as_dict
from calibre.db.backend import DB
from calibre.db.cache import Cache
from calibre.db.categories import Tag
from calibre.db.errors import NoSuchFormat
from calibre.db.view import View
from calibre.ebooks.metadata import author_to_author_sort, string_to_authors, title_sort
from calibre.ebooks.metadata.book.base import Metadata
from calibre.utils.date import UNDEFINED_DATE, utcnow


def _db_file(path):
    return os.path.join(path, 'metadata.db')


class LibraryDatabase:
    """The legacy library API. `new_api` is the `Cache`."""

    PATH_LIMIT = DB.PATH_LIMIT
    WINDOWS_LIBRARY_PATH_LIMIT = DB.WINDOWS_LIBRARY_PATH_LIMIT
    CATEGORY_SORTS = ('name', 'popularity', 'rating')
    MATCH_TYPE = ('any', 'all')
    CUSTOM_DATA_TYPES = frozenset(
        ['rating', 'text', 'comments', 'datetime', 'int', 'float', 'bool', 'series', 'composite', 'enumeration']
    )

    @classmethod
    def exists_at(cls, path):
        return path and os.path.exists(_db_file(path))

    def __init__(
        self,
        library_path,
        default_prefs=None,
        read_only=False,
        is_second_db=False,
        progress_callback=lambda x, y: True,
        restore_all_prefs=False,
        row_factory=False,
        temp_db=False,
    ):
        self.is_second_db = is_second_db
        self.listeners = set()
        backend = self.backend = DB(
            library_path,
            default_prefs=default_prefs,
            read_only=read_only,
            restore_all_prefs=restore_all_prefs,
            progress_callback=progress_callback,
            temp_db=temp_db,
        )
        cache = self.new_api = Cache(backend, library_database_instance=self)
        cache.init()
        self.data = View(cache)
        self.id = self.data.index_to_id
        self.row = self.data.id_to_index
        self.index = self.data.id_to_index
        for x in (
            'get_property',
            'count',
            'refresh_ids',
            'set_marked_ids',
            'multisort',
            'search',
            'search_getting_ids',
            'get_search_restriction',
            'set_search_restriction',
            'get_base_restriction',
            'set_base_restriction',
            'get_base_restriction_name',
            'set_base_restriction_name',
            'get_search_restriction_name',
            'set_search_restriction_name',
            'search_restriction_applied',
            'get_search_restriction_book_count',
            'books_added',
            'books_deleted',
            'toggle_marked_ids',
            'get_marked',
        ):
            setattr(self, x, getattr(self.data, x, None))
        self.data.refresh()
        self.last_update_check = self.last_modified()
        self.book_on_device_func = None
        self.prefs = backend.prefs
        self.field_metadata = backend.field_metadata
        self.FIELD_MAP = self.data.FIELD_MAP
        self.has_id = cache.has_id
        self.format_abspath = self._format_abspath

    def close(self):
        self.new_api.close()

    def break_cycles(self):
        pass

    def __len__(self):
        return len(self.data)

    def __iter__(self):
        yield from self.data

    @property
    def conn(self):
        return self.backend.conn

    @property
    def library_path(self):
        return self.backend.library_path

    @property
    def dbpath(self):
        return self.backend.dbpath

    @property
    def library_id(self):
        return self.backend.library_id

    @property
    def custom_column_label_map(self):
        return self.backend.custom_column_label_map

    @property
    def custom_column_num_map(self):
        return self.backend.custom_column_num_map

    @property
    def is_closed(self):
        return self.backend.is_closed

    def commit(self):
        self.backend.commit()

    def refresh(self, field=None, ascending=True):
        self.data.refresh(field=field, ascending=ascending)
        self.backend.initialize_custom_columns()
        self.data.build_field_map()
        self.FIELD_MAP = self.data.FIELD_MAP

    def notify(self, event, ids=[]):
        for listener in self.listeners:
            with suppress(Exception):
                listener(event, ids)

    def add_listener(self, listener):
        self.listeners.add(listener)

    def last_modified(self):
        return self.new_api.last_modified()

    def check_if_modified(self):
        if self.last_modified() > self.last_update_check:
            self.refresh()
        self.last_update_check = utcnow()

    def path(self, index, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.field_for('path', book_id).replace('/', os.sep)

    def abspath(self, index, index_is_id=False, create_dirs=True):
        path = os.path.join(self.library_path, self.path(index, index_is_id=index_is_id))
        if create_dirs and not os.path.exists(path):
            os.makedirs(path)
        return path

    # -- Single-field getters --

    def _field(self, name, index, index_is_id=False, default=None):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.field_for(name, book_id, default_value=default)

    def title(self, index, index_is_id=False):
        return self._field('title', index, index_is_id, 'Unknown')

    def title_sort(self, index, index_is_id=False):
        return self._field('sort', index, index_is_id)

    def authors(self, index, index_is_id=False):
        a = self._field('authors', index, index_is_id)
        return ','.join(x.replace(',', '|') for x in a) if a else None

    def author_sort(self, index, index_is_id=False):
        return self._field('author_sort', index, index_is_id)

    def publisher(self, index, index_is_id=False):
        return self._field('publisher', index, index_is_id)

    def rating(self, index, index_is_id=False):
        return self._field('rating', index, index_is_id)

    def series(self, index, index_is_id=False):
        return self._field('series', index, index_is_id)

    def series_index(self, index, index_is_id=False):
        return self._field('series_index', index, index_is_id, 1.0)

    def tags(self, index, index_is_id=False):
        t = self._field('tags', index, index_is_id)
        return ','.join(t) if t else None

    def comments(self, index, index_is_id=False):
        return self._field('comments', index, index_is_id)

    def pubdate(self, index, index_is_id=False):
        return self._field('pubdate', index, index_is_id, UNDEFINED_DATE)

    def timestamp(self, index, index_is_id=False):
        return self._field('timestamp', index, index_is_id, UNDEFINED_DATE)

    def uuid(self, index, index_is_id=False):
        return self._field('uuid', index, index_is_id)

    def isbn(self, index, index_is_id=False):
        return (self._field('identifiers', index, index_is_id, {}) or {}).get('isbn')

    def languages(self, index, index_is_id=False):
        return self._field('languages', index, index_is_id)

    def book_on_device(self, book_id):
        if callable(self.book_on_device_func):
            return self.book_on_device_func(book_id)
        return None

    def book_on_device_string(self, book_id):
        return ''

    def set_book_on_device_func(self, func):
        self.book_on_device_func = func

    def get_identifiers(self, index, index_is_id=False):
        return dict(self._field('identifiers', index, index_is_id, {}) or {})

    def get_field(self, index, key, default=None, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        mi = self.new_api.get_metadata(book_id, get_cover=key == 'cover')
        return mi.get(key, default)

    def metadata_for_field(self, key):
        return self.field_metadata.get(key)

    def has_cover(self, book_id):
        return self.new_api.has_cover(book_id)

    def cover(self, index, index_is_id=False, as_file=False, as_image=False, as_path=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.cover(book_id, as_file=as_file, as_image=as_image, as_path=as_path)

    def copy_cover_to(self, index, dest, index_is_id=False, windows_atomic_move=None, use_hardlink=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.copy_cover_to(book_id, dest, use_hardlink=use_hardlink)

    def cover_last_modified(self, index, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.cover_last_modified(book_id)

    def set_cover(self, book_id, data, notify=True, commit=True):
        self.new_api.set_cover({book_id: data})
        if notify:
            self.notify('cover', [book_id])

    def remove_cover(self, book_id, notify=True, commit=True):
        self.new_api.set_cover({book_id: None})
        if notify:
            self.notify('cover', [book_id])

    # -- Formats --

    def formats(self, index, index_is_id=False, verify_formats=True):
        book_id = index if index_is_id else self.id(index)
        f = self.new_api.formats(book_id, verify_formats=verify_formats)
        return ','.join(f) if f else None

    def has_format(self, index, fmt, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.has_format(book_id, fmt)

    def _format_abspath(self, index, fmt, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.format_abspath(book_id, fmt)

    def format_files(self, index, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        return [(name, fmt) for name, fmt in self.new_api.format_files(book_id).items()]

    def format_metadata(self, book_id, fmt, allow_cache=True, update_db=False, commit=False):
        return self.new_api.format_metadata(book_id, fmt, allow_cache=allow_cache, update_db=update_db)

    def format_last_modified(self, book_id, fmt):
        m = self.format_metadata(book_id, fmt)
        if m:
            return m['mtime']

    def format_hash(self, book_id, fmt):
        return self.new_api.format_hash(book_id, fmt)

    def format_path(self, index, fmt, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        ans = self.new_api.format_abspath(book_id, fmt)
        if ans is None:
            raise NoSuchFormat('Record %d has no format: %s' % (book_id, fmt))
        return ans

    def format(self, index, fmt, index_is_id=False, as_file=False, mode='r+b', as_path=False, preserve_filename=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.format(book_id, fmt, as_file=as_file, as_path=as_path, preserve_filename=preserve_filename)

    def copy_format_to(self, index, fmt, dest, index_is_id=False, windows_atomic_move=None, use_hardlink=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.copy_format_to(book_id, fmt, dest, use_hardlink=use_hardlink)

    def add_format(
        self, index, fmt, stream, index_is_id=False, path=None, notify=True, replace=True, copy_function=None
    ):
        book_id = index if index_is_id else self.id(index)
        ret = self.new_api.add_format(book_id, fmt, stream, replace=replace, run_hooks=False, dbapi=self)
        if ret and notify:
            self.notify('metadata', [book_id])
        return ret

    def add_format_with_hooks(self, index, fmt, fpath, index_is_id=False, path=None, notify=True, replace=True):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.add_format(book_id, fmt, fpath, replace=replace, run_hooks=True, dbapi=self)

    def remove_format(self, index, fmt, index_is_id=False, notify=True, commit=True, db_only=False):
        book_id = index if index_is_id else self.id(index)
        self.new_api.remove_formats({book_id: (fmt,)}, db_only=db_only)
        if notify:
            self.notify('metadata', [book_id])

    def max_size(self, index, index_is_id=False):
        return self._field('size', index, index_is_id)

    def sizeof_format(self, index, fmt, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.format_sizes(book_id).get(fmt.upper())

    # -- Metadata --

    def get_metadata(self, index, index_is_id=False, get_cover=False, get_user_categories=True, cover_as_data=False):
        book_id = index if index_is_id else self.id(index)
        return self.new_api.get_metadata(
            book_id, get_cover=get_cover, get_user_categories=get_user_categories, cover_as_data=cover_as_data
        )

    def get_proxy_metadata(self, book_id):
        return self.new_api.get_proxy_metadata(book_id)

    def set_metadata(
        self,
        book_id,
        mi,
        ignore_errors=False,
        set_title=True,
        set_authors=True,
        commit=True,
        force_changes=False,
        notify=True,
    ):
        self.new_api.set_metadata(
            book_id,
            mi,
            ignore_errors=ignore_errors,
            set_title=set_title,
            set_authors=set_authors,
            force_changes=force_changes,
        )
        if notify:
            self.notify('metadata', [book_id])

    def _set(self, field, book_id, val, notify=True, **kw):
        self.new_api.set_field(field, {book_id: val})
        if notify:
            self.notify('metadata', [book_id])

    def set_title(self, book_id, title, notify=True, commit=True):
        self._set('title', book_id, title, notify)

    def set_title_sort(self, book_id, title_sort_, notify=True, commit=True):
        self._set('sort', book_id, title_sort_, notify)

    def set_authors(self, book_id, authors, notify=True, commit=True, allow_case_change=False):
        self._set('authors', book_id, authors, notify)

    def set_author_sort(self, book_id, sort, notify=True, commit=True):
        self._set('author_sort', book_id, sort, notify)

    def set_series(self, book_id, series, notify=True, commit=True, allow_case_change=True):
        self._set('series', book_id, series, notify)

    def set_series_index(self, book_id, idx, notify=True, commit=True):
        self._set('series_index', book_id, idx, notify)

    def set_tags(self, book_id, tags, append=False, notify=True, commit=True, allow_case_change=False):
        if isinstance(tags, str):
            tags = [t.strip() for t in tags.split(',')]
        if append:
            old = list(self.new_api.field_for('tags', book_id, ()) or ())
            lower = {t.lower() for t in old}
            tags = old + [t for t in tags if t.lower() not in lower]
        self._set('tags', book_id, tags, notify)

    def set_publisher(self, book_id, publisher, notify=True, commit=True, allow_case_change=False):
        self._set('publisher', book_id, publisher, notify)

    def set_rating(self, book_id, rating, notify=True, commit=True):
        self._set('rating', book_id, rating, notify)

    def set_comment(self, book_id, text, notify=True, commit=True):
        self._set('comments', book_id, text, notify)

    set_comments = set_comment

    def set_pubdate(self, book_id, dt, notify=True, commit=True):
        self._set('pubdate', book_id, dt, notify)

    def set_timestamp(self, book_id, dt, notify=True, commit=True):
        self._set('timestamp', book_id, dt, notify)

    def set_languages(self, book_id, languages, notify=True, commit=True):
        self._set('languages', book_id, languages, notify)

    def set_uuid(self, book_id, uuid, notify=True, commit=True):
        self._set('uuid', book_id, uuid, notify)

    def set_identifiers(self, book_id, identifiers, notify=True, commit=True):
        self._set('identifiers', book_id, identifiers, notify)

    def set_identifier(self, book_id, typ, val, notify=True, commit=True):
        ids = self.get_identifiers(book_id, index_is_id=True)
        if val:
            ids[typ.lower()] = val
        else:
            ids.pop(typ.lower(), None)
        self._set('identifiers', book_id, ids, notify)

    def set_isbn(self, book_id, isbn, notify=True, commit=True):
        self.set_identifier(book_id, 'isbn', isbn, notify=notify)

    def set_path(self, index, index_is_id=False):
        pass

    def dirtied(self, book_ids, commit=True):
        self.new_api.dirtied(book_ids)

    def dirty_queue_length(self):
        return 0

    def commit_dirty_cache(self):
        pass

    # -- Custom columns --

    def custom_field_keys(self, include_composites=True):
        return self.field_metadata.custom_field_keys(include_composites=include_composites)

    def custom_field_name(self, label=None, num=None):
        return self.backend.custom_field_name(label, num)

    def custom_field_metadata(self, label=None, num=None):
        if label is None and num is None:
            return self.field_metadata.custom_field_metadata()
        return self.backend.custom_field_metadata(label, num)

    def _custom_key(self, label=None, num=None):
        if label is None:
            label = self.backend.custom_column_num_map[num]['label']
        return '#' + label

    def get_custom(self, index, label=None, num=None, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        key = self._custom_key(label, num)
        data = self.backend.custom_column_label_map[key[1:]]
        ret = self.new_api.field_for(key, book_id)
        if data['datatype'] == 'text' and data['is_multiple']:
            ret = list(ret) if ret else []
        return ret

    def get_custom_extra(self, index, label=None, num=None, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        key = self._custom_key(label, num)
        data = self.backend.custom_column_label_map[key[1:]]
        if data['datatype'] != 'series':
            return None
        return self.new_api.field_for(key + '_index', book_id)

    def get_custom_and_extra(self, index, label=None, num=None, index_is_id=False):
        return self.get_custom(index, label, num, index_is_id), self.get_custom_extra(index, label, num, index_is_id)

    def get_next_cc_series_num_for(self, series, label=None, num=None):
        key = self._custom_key(label, num)
        return self.new_api.get_next_series_num_for(series, field=key)

    def is_item_used_in_multiple(self, item, label=None, num=None):
        key = self._custom_key(label, num)
        item_id = self.new_api.get_item_id(key, item)
        return item_id is not None and len(self.new_api.books_for_field(key, item_id)) > 1

    def delete_custom_item_using_id(self, item_id, label=None, num=None):
        key = self._custom_key(label, num)
        self.new_api.remove_items(key, (item_id,))

    def rename_custom_item(self, old_id, new_name, label=None, num=None):
        key = self._custom_key(label, num)
        self.new_api.rename_items(key, {old_id: new_name})

    def delete_item_from_multiple(self, item, label=None, num=None):
        key = self._custom_key(label, num)
        item_id = self.new_api.get_item_id(key, item)
        if item_id is not None:
            self.new_api.remove_items(key, (item_id,))

    def set_custom(
        self,
        book_id,
        val,
        label=None,
        num=None,
        append=False,
        notify=True,
        extra=None,
        commit=True,
        allow_case_change=False,
    ):
        key = self._custom_key(label, num)
        data = self.backend.custom_column_label_map[key[1:]]
        if data['datatype'] == 'text' and data['is_multiple']:
            if isinstance(val, str):
                val = [x.strip() for x in val.split(data['multiple_seps'].get('ui_to_list', ','))]
            if append:
                old = list(self.new_api.field_for(key, book_id, ()) or ())
                lower = {x.lower() for x in old}
                val = old + [x for x in (val or []) if x.lower() not in lower]
        if data['datatype'] == 'series':
            if extra is None:
                sval, sidx = _get_series_values(val) if isinstance(val, str) else (val, None)
                if sidx is not None:
                    val, extra = sval, sidx
                else:
                    extra = self.new_api.field_for(key + '_index', book_id) if val else None
                    if extra is None and val:
                        extra = self.get_next_cc_series_num_for(val, label=key[1:])
            val = (val, extra) if val else None
        self.new_api.set_field(key, {book_id: val})
        if notify:
            self.notify('metadata', [book_id])
        return set()

    def set_custom_bulk(self, ids, val, label=None, num=None, append=False, notify=True, extras=None):
        for i, book_id in enumerate(ids):
            extra = extras[i] if extras else None
            self.set_custom(book_id, val, label=label, num=num, append=append, notify=False, extra=extra)
        if notify:
            self.notify('metadata', list(ids))

    def all_custom(self, label=None, num=None):
        key = self._custom_key(label, num)
        return self.new_api.all_field_names(key)

    def get_custom_items_with_ids(self, label=None, num=None):
        key = self._custom_key(label, num)
        return list(self.new_api.get_id_map(key).items())

    def create_custom_column(self, label, name, datatype, is_multiple, editable=True, display={}):
        num = self.new_api.create_custom_column(label, name, datatype, is_multiple, editable=editable, display=display)
        self.refresh()
        return num

    def delete_custom_column(self, label=None, num=None):
        self.new_api.delete_custom_column(label=label, num=num)

    def set_custom_column_metadata(
        self, num, name=None, label=None, is_editable=None, display=None, notify=True, update_last_modified=False
    ):
        changed = self.new_api.set_custom_column_metadata(
            num, name=name, label=label, is_editable=is_editable, display=display
        )
        if changed:
            self.refresh()
        return changed

    def custom_table_names(self, num):
        return self.backend.custom_table_names(num)

    # -- Items --

    def all_ids(self):
        return list(self.new_api.all_book_ids())

    def is_empty(self):
        return not self.new_api.all_book_ids()

    def all_tags(self):
        return sorted(self.new_api.all_field_names('tags'))

    def all_authors(self):
        return list(self.new_api.get_id_map('authors').items())

    def all_series(self):
        return list(self.new_api.get_id_map('series').items())

    def all_publishers(self):
        return list(self.new_api.get_id_map('publisher').items())

    def all_tag_names(self):
        return self.all_tags()

    def all_author_names(self):
        return list(self.new_api.all_field_names('authors'))

    def all_series_names(self):
        return list(self.new_api.all_field_names('series'))

    def all_publisher_names(self):
        return list(self.new_api.all_field_names('publisher'))

    def all_formats(self):
        ans = set()
        for b in self.new_api.all_book_ids():
            ans |= set(self.new_api.field_for('formats', b, ()) or ())
        return ans

    def all_languages(self):
        return list(self.new_api.get_id_map('languages').items())

    def get_tags_with_ids(self):
        return list(self.new_api.get_id_map('tags').items())

    def get_authors_with_ids(self):
        return [(i, d['name'], d['sort'], d['link']) for i, d in self.new_api.author_data().items()]

    def get_series_with_ids(self):
        return list(self.new_api.get_id_map('series').items())

    def get_publishers_with_ids(self):
        return list(self.new_api.get_id_map('publisher').items())

    def get_ratings_with_ids(self):
        return list(self.new_api.get_id_map('rating').items())

    def get_languages_with_ids(self):
        return list(self.new_api.get_id_map('languages').items())

    def authors_with_sort_strings(self, index, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        ids = self.new_api.field_ids_for('authors', book_id)
        data = self.new_api.author_data(ids)
        return [
            (i, data[i]['name'], data[i]['sort'] or author_to_author_sort(data[i]['name']), data[i]['link'])
            for i in ids
            if i in data
        ]

    def author_sort_from_authors(self, authors):
        return self.new_api.author_sort_from_authors(authors)

    def author_sort_from_book(self, index, index_is_id=False):
        return self.author_sort_from_authors(self._field('authors', index, index_is_id, ()))

    def author_id(self, index, index_is_id=False):
        ids = self.new_api.field_ids_for('authors', index if index_is_id else self.id(index))
        return ids[0] if ids else None

    def set_sort_field_for_author(self, old_id, new_sort, commit=True, notify=False):
        self.new_api.set_sort_for_authors({old_id: new_sort})

    def set_link_field_for_author(self, aid, link, commit=True, notify=False):
        self.new_api.set_link_for_authors({aid: link})

    def rename_author(self, old_id, new_name):
        self.new_api.rename_items('authors', {old_id: new_name})

    def rename_tag(self, old_id, new_name):
        self.new_api.rename_items('tags', {old_id: new_name})

    def rename_series(self, old_id, new_name, change_index=True):
        self.new_api.rename_items('series', {old_id: new_name}, change_index=change_index)

    def rename_publisher(self, old_id, new_name):
        self.new_api.rename_items('publisher', {old_id: new_name})

    def delete_tag_using_id(self, tag_id):
        self.new_api.remove_items('tags', (tag_id,))

    def delete_series_using_id(self, series_id):
        self.new_api.remove_items('series', (series_id,))

    def delete_publisher_using_id(self, publisher_id):
        self.new_api.remove_items('publisher', (publisher_id,))

    def delete_tags(self, tags):
        ids = [self.new_api.get_item_id('tags', t) for t in tags]
        self.new_api.remove_items('tags', [i for i in ids if i is not None])

    def unapply_tags(self, book_id, tags, notify=True):
        old = list(self.new_api.field_for('tags', book_id, ()) or ())
        lower = {t.lower() for t in tags}
        self._set('tags', book_id, [t for t in old if t.lower() not in lower], notify)

    def bulk_modify_tags(self, ids, add=[], remove=[], notify=False):
        for book_id in ids:
            old = list(self.new_api.field_for('tags', book_id, ()) or ())
            lower_remove = {t.lower() for t in remove}
            new = [t for t in old if t.lower() not in lower_remove]
            lower = {t.lower() for t in new}
            new += [t for t in add if t.lower() not in lower]
            self.new_api.set_field('tags', {book_id: new})
        if notify:
            self.notify('metadata', list(ids))

    def tags_older_than(self, tag, delta, must_have_tag=None, must_have_authors=None):
        return self.new_api.tags_older_than(
            tag, delta=delta, must_have_tag=must_have_tag, must_have_authors=must_have_authors
        )

    def get_usage_count_by_id(self, field):
        return list(self.new_api.get_usage_count_by_id(field).items())

    def get_categories(self, sort='name', ids=None, icon_map=None):
        return self.new_api.get_categories(sort=sort, book_ids=ids)

    def books_in_series(self, series_id):
        books = self.new_api.books_for_field('series', series_id)
        return sorted(books, key=lambda b: self.new_api.field_for('series_index', b, 1.0))

    def books_in_series_of(self, index, index_is_id=False):
        book_id = index if index_is_id else self.id(index)
        ids = self.new_api.field_ids_for('series', book_id)
        return self.books_in_series(ids[0]) if ids else []

    def books_with_same_title(self, mi, all_matches=True):
        return self.new_api.books_with_same_title(mi, all_matches=all_matches)

    def get_next_series_num_for(self, series):
        return self.new_api.get_next_series_num_for(series)

    def has_book(self, mi):
        return self.new_api.has_book(mi)

    def find_identical_books(self, mi):
        return self.new_api.find_identical_books(mi)

    def get_id_from_uuid(self, uuid):
        row = self.conn.execute('SELECT id FROM books WHERE uuid=?', (uuid,)).fetchone()
        return row[0] if row else None

    def get_ids_for_custom_book_data(self, name):
        return self.new_api.get_ids_for_custom_book_data(name)

    def add_custom_book_data(self, book_id, name, val):
        self.new_api.add_custom_book_data(name, {book_id: val})

    def add_multiple_custom_book_data(self, name, vals, delete_first=False):
        self.new_api.add_custom_book_data(name, vals, delete_first=delete_first)

    def get_custom_book_data(self, book_id, name, default=None):
        return self.new_api.get_custom_book_data(name, (book_id,), default).get(book_id, default)

    def get_all_custom_book_data(self, name, default=None):
        return self.new_api.get_custom_book_data(name, (), default)

    def delete_custom_book_data(self, book_id, name):
        self.new_api.delete_custom_book_data(name, (book_id,))

    def delete_all_custom_book_data(self, name):
        self.new_api.delete_custom_book_data(name, ())

    def conversion_options(self, book_id, fmt):
        return self.new_api.conversion_options(book_id, fmt)

    def has_conversion_options(self, ids, fmt='PIPE'):
        return self.new_api.has_conversion_options(ids, fmt)

    def delete_conversion_options(self, book_id, fmt, commit=True):
        self.new_api.delete_conversion_options((book_id,), fmt)

    def set_conversion_options(self, book_id, fmt, options):
        self.new_api.set_conversion_options({book_id: options}, fmt)

    # -- Adding and removing --

    def create_book_entry(self, mi, cover=None, add_duplicates=True, force_id=None):
        book_id = self.new_api.create_book_entry(mi, cover=cover, add_duplicates=add_duplicates, force_id=force_id)
        if book_id is not None:
            self.data.books_added((book_id,))
        return book_id

    def import_book(
        self, mi, formats, notify=True, import_hooks=True, apply_import_tags=True, preserve_uuid=False, run_hooks=True
    ):
        format_map = {}
        for path in formats:
            ext = os.path.splitext(path)[1][1:].upper()
            if ext == 'OPF':
                continue
            format_map[ext] = path
        book_ids, duplicates = self.new_api.add_books(
            [(mi, format_map)],
            add_duplicates=True,
            apply_import_tags=apply_import_tags,
            preserve_uuid=preserve_uuid,
            run_hooks=import_hooks and run_hooks,
            dbapi=self,
        )
        if book_ids:
            self.data.books_added(book_ids)
            if notify:
                self.notify('add', book_ids)
            return book_ids[0]

    def add_books(self, paths, formats, metadata, add_duplicates=True, return_ids=False):
        books = [(mi, {fmt.upper(): path}) for mi, path, fmt in zip(metadata, paths, formats)]
        book_ids, duplicates = self.new_api.add_books(books, add_duplicates=add_duplicates, dbapi=self)
        if duplicates:
            paths, formats, metadata = [], [], []
            for mi, format_map in duplicates:
                metadata.append(mi)
                for fmt, path in format_map.items():
                    formats.append(fmt)
                    paths.append(path)
            duplicates = (paths, formats, metadata)
        self.data.books_added(book_ids)
        if book_ids:
            self.notify('add', book_ids)
        return (book_ids if return_ids else len(book_ids)), (duplicates or None)

    def delete_book(self, book_id, notify=True, commit=True, permanent=False, do_clean=True):
        self.new_api.remove_books((book_id,), permanent=permanent)
        self.data.books_deleted((book_id,))
        if notify:
            self.notify('delete', [book_id])

    def remove_books(self, book_ids, permanent=False):
        self.new_api.remove_books(book_ids, permanent=permanent)
        self.data.books_deleted(tuple(book_ids))

    # -- Searching --

    def get_data_as_dict(self, prefix=None, authors_as_string=False, ids=None, convert_to_local_tz=True):
        return get_data_as_dict(
            self, prefix=prefix, authors_as_string=authors_as_string, ids=ids, convert_to_local_tz=convert_to_local_tz
        )

    def sort(self, field, ascending, subsort=False):
        self.data.sort(field, ascending, subsort=subsort)

    def clean(self):
        pass

    def clean_custom(self):
        pass

    def check_integrity(self):
        pass

    def vacuum(self):
        self.new_api.vacuum()

    def set_saved_searches(self, saved_searches):
        pass

    @property
    def saved_search_names(self):
        return sorted(self.backend.prefs.get('saved_searches', {}) or {})

    def saved_search_lookup(self, name):
        return (self.backend.prefs.get('saved_searches', {}) or {}).get(name)

    def saved_search_add(self, name, val):
        d = dict(self.backend.prefs.get('saved_searches', {}) or {})
        d[name] = val
        self.backend.prefs['saved_searches'] = d

    def saved_search_delete(self, name):
        d = dict(self.backend.prefs.get('saved_searches', {}) or {})
        d.pop(name, None)
        self.backend.prefs['saved_searches'] = d

    def saved_search_set_all(self, smap):
        self.backend.prefs['saved_searches'] = dict(smap)

    def saved_search_rename(self, old_name, new_name):
        d = dict(self.backend.prefs.get('saved_searches', {}) or {})
        if old_name in d:
            d[new_name] = d.pop(old_name)
            self.backend.prefs['saved_searches'] = d

    def saved_search_names_sorted(self):
        return self.saved_search_names

    def get_search_restriction(self):
        return self.data.get_search_restriction()

    def __getattr__(self, name):
        if name.startswith('__'):
            raise AttributeError(name)
        api = self.__dict__.get('new_api')
        if api is not None and hasattr(api, name):
            return getattr(api, name)
        raise AttributeError(f'LibraryDatabase has no attribute {name!r}')


LibraryDatabase2 = LibraryDatabase

__all__ = [
    'LibraryDatabase',
    'LibraryDatabase2',
    'Tag',
    'Metadata',
    'string_to_authors',
    'title_sort',
    'traceback',
    '_get_next_series_num_for_list',
]
