# Derived from calibre's src/calibre/db/view.py (calibre 9.15).
# Copyright: 2011, Kovid Goyal <kovid@kovidgoyal.net>. License: GPLv3.
# Modified for Magnitide, 2026.
"""Mirrors calibre's `calibre.db.view` (calibre 9.15): the row-oriented view
of the library (`db.data`), where plugins written for the old API index
books by row: `db.data[row][db.FIELD_MAP['title']]`, `db.id(row)`,
`db.row(book_id)`."""

from functools import partial

from calibre.utils.icu import sort_key


def sanitize_sort_field_name(field_metadata, field):
    field = field_metadata.search_term_to_field_key(field.lower().strip())
    # translate some fields to their hidden equivalent
    field = {'title': 'sort', 'authors': 'author_sort'}.get(field, field)
    return field


class TableRow:
    def __init__(self, book_id, view):
        self.book_id = book_id
        self.view = view
        self.column_count = view.column_count

    def __getitem__(self, obj):
        view = self.view
        if isinstance(obj, slice):
            return [view._field_getters[c](self.book_id) for c in range(*obj.indices(len(view._field_getters)))]
        try:
            return view._field_getters[obj](self.book_id)
        except KeyError:
            raise IndexError('Invalid column index: %r' % obj)

    def __len__(self):
        self._sync()
        return self.column_count

    def __iter__(self):
        self._sync()
        for i in range(self.column_count):
            yield self[i]


class View:
    def __init__(self, cache):
        self.cache = cache
        self.marked_ids = {}
        self.marked_listeners = {}
        self.search_restriction_book_count = 0
        self.search_restriction = self.base_restriction = ''
        self.search_restriction_name = self.base_restriction_name = ''
        self._field_getters = {}
        self.column_count = len(cache.backend.FIELD_MAP) if hasattr(cache.backend, 'FIELD_MAP') else 0
        self.build_field_map()
        self._map = tuple(sorted(cache.all_book_ids()))
        self._map_filtered = self._map
        self.full_map_is_sorted = True
        self.sort_history = [('id', True)]

    def build_field_map(self):
        self.FIELD_MAP = {
            'id': 0,
            'title': 1,
            'authors': 2,
            'timestamp': 3,
            'size': 4,
            'rating': 5,
            'tags': 6,
            'comments': 7,
            'series': 8,
            'publisher': 9,
            'series_index': 10,
            'sort': 11,
            'author_sort': 12,
            'formats': 13,
            'path': 14,
            'pubdate': 15,
            'uuid': 16,
            'cover': 17,
            'au_map': 18,
            'last_modified': 19,
            'identifiers': 20,
            'languages': 21,
        }
        n = 22
        for key in self.cache.custom_field_keys():
            self.FIELD_MAP[key] = n
            n += 1
            if self.cache.field_metadata[key]['datatype'] == 'series':
                self.FIELD_MAP[key + '_index'] = n
                n += 1
        self.column_count = n
        self._field_getters = {}
        for col, idx in self.FIELD_MAP.items():
            self._field_getters[idx] = partial(self._value, col)
        self.FIELD_MAP['ondevice'] = n
        self._field_getters[n] = lambda book_id: None
        self.FIELD_MAP['marked'] = n + 1
        self._field_getters[n + 1] = lambda book_id: self.marked_ids.get(book_id)
        self.column_count = n + 2

    def _value(self, col, book_id):
        if col == 'id':
            return book_id
        v = self.cache.field_for(col, book_id)
        if col in ('tags', 'languages', 'formats') and v is not None:
            v = ','.join(v)
        elif col == 'authors' and v is not None:
            v = ' & '.join(v)
        elif col == 'identifiers' and v is not None:
            v = ','.join(f'{k}:{x}' for k, x in v.items())
        elif col.startswith('#') and isinstance(v, tuple):
            v = ','.join(v)
        return v

    @property
    def field_metadata(self):
        return self.cache.field_metadata

    def _sync(self):
        """Books added or removed through `new_api` (not this view) join or
        leave the map, keeping its order."""
        current = self.cache.all_book_ids()
        if len(current) == len(self._map) and current.issuperset(self._map):
            return
        known = set(self._map)
        gone = known - current
        new = sorted(current - known)
        if gone:
            self._map = tuple(i for i in self._map if i not in gone)
            self._map_filtered = tuple(i for i in self._map_filtered if i not in gone)
        if new:
            self._map = tuple(new) + self._map
            self._map_filtered = tuple(new) + self._map_filtered

    def _get_id(self, idx, index_is_id=True):
        if index_is_id and not self.cache.has_id(idx):
            raise IndexError('No book with id %s present' % idx)
        return idx if index_is_id else self.index_to_id(idx)

    def has_id(self, book_id):
        return self.cache.has_id(book_id)

    def __getitem__(self, row):
        self._sync()
        return TableRow(self._map_filtered[row], self)

    def __len__(self):
        return len(self._map_filtered)

    def __iter__(self):
        for book_id in self._map_filtered:
            yield TableRow(book_id, self)

    def iterall(self):
        for book_id in self.iterallids():
            yield TableRow(book_id, self)

    def iterallids(self):
        self._sync()
        yield from sorted(self._map)

    def tablerow_for_id(self, book_id):
        return TableRow(book_id, self)

    def get_field_map_field(self, row, col, index_is_id=True):
        idx = col if isinstance(col, int) else self.FIELD_MAP[col]
        book_id = row if index_is_id else self.index_to_id(row)
        return self._field_getters[idx](book_id)

    def index_to_id(self, idx):
        return self._map_filtered[idx]

    def id(self, idx):
        return self._map_filtered[idx]

    def id_to_index(self, book_id):
        self._sync()
        return self._map_filtered.index(book_id)

    row = index = id_to_index

    def index_to_id(self, idx):  # noqa: F811
        self._sync()
        return self._map_filtered[idx]

    def __getattr__(self, name):
        if name.startswith('__'):
            raise AttributeError(name)
        # Legacy `self.data.<field>(idx, index_is_id=False)` getters.
        if name in self.FIELD_MAP:

            def getter(idx, index_is_id=False):
                return self._value(name, idx if index_is_id else self.index_to_id(idx))

            return getter
        raise AttributeError(name)

    def get(self, row, col, row_is_id=False):
        book_id = row if row_is_id else self._map_filtered[row]
        return self._field_getters[col](book_id)

    def refresh(self, field=None, ascending=True, clear_caches=True, do_search=True):
        self._map = tuple(sorted(self.cache.all_book_ids()))
        self._map_filtered = self._map
        self.cache.clear_search_caches()
        if field is not None:
            self.sort(field, ascending)
        if self.search_restriction or self.base_restriction:
            self.search('', return_matches=False)

    def refresh_ids(self, ids):
        return [self.id_to_index(i) for i in ids if i in self._map_filtered]

    def remove(self, book_id):
        self._map = tuple(i for i in self._map if i != book_id)
        self._map_filtered = tuple(i for i in self._map_filtered if i != book_id)

    def books_added(self, ids):
        ids = tuple(ids)
        self._map = ids + self._map
        self._map_filtered = ids + self._map_filtered

    def books_deleted(self, ids):
        for i in ids:
            self.remove(i)

    def count(self):
        self._sync()
        return len(self._map)

    def sort(self, field, ascending, subsort=False):
        self.multisort([(field, ascending)])

    def multisort(self, fields=[], subsort=False, only_ids=None):
        fields = [(sanitize_sort_field_name(self.field_metadata, x), bool(y)) for x, y in fields]
        keys = self.field_metadata.sortable_field_keys()
        fields = [x for x in fields if x[0] in keys]
        if subsort and 'sort' not in [x[0] for x in fields]:
            fields += [('sort', True)]
        if not fields:
            fields = [('timestamp', False)]
        sorted_book_ids = self.cache.multisort(fields, ids_to_sort=only_ids)
        if only_ids is None:
            self._map = tuple(sorted_book_ids)
            if len(self._map_filtered) == len(self._map):
                self._map_filtered = tuple(self._map)
            else:
                fids = frozenset(self._map_filtered)
                self._map_filtered = tuple(i for i in self._map if i in fids)
            self.full_map_is_sorted = True
            self.sort_history = [tuple(x) for x in fields] + self.sort_history
        else:
            smap = {book_id: i for i, book_id in enumerate(sorted_book_ids)}
            only_ids.sort(key=smap.get)

    def search(self, query, return_matches=False, sort_results=True):
        ans = self.search_getting_ids(
            query, self.search_restriction, set_restriction_count=True, sort_results=sort_results
        )
        if return_matches:
            return ans
        self._map_filtered = tuple(ans)

    def _build_restriction_string(self, restriction):
        if self.base_restriction:
            if restriction:
                return '(%s) and (%s)' % (self.base_restriction, restriction)
            return self.base_restriction
        return restriction

    def search_getting_ids(
        self, query, search_restriction, set_restriction_count=False, use_virtual_library=True, sort_results=True
    ):
        self._sync()
        if use_virtual_library:
            search_restriction = self._build_restriction_string(search_restriction)
        q = ''
        if not query or not query.strip():
            q = search_restriction
        else:
            q = query
            if search_restriction:
                q = '(%s) and (%s)' % (search_restriction, query)
        if not q:
            if set_restriction_count:
                self.search_restriction_book_count = len(self._map)
            return list(self._map)
        matches = self.cache.search(query, search_restriction)
        if len(matches) == len(self._map):
            rv = list(self._map)
        else:
            rv = [x for x in self._map if x in matches]
        if set_restriction_count and q == search_restriction:
            self.search_restriction_book_count = len(rv)
        return rv

    def get_search_restriction(self):
        return self.search_restriction

    def set_search_restriction(self, s):
        self.search_restriction = s

    def get_base_restriction(self):
        return self.base_restriction

    def set_base_restriction(self, s):
        self.base_restriction = s

    def get_base_restriction_name(self):
        return self.base_restriction_name

    def set_base_restriction_name(self, s):
        self.base_restriction_name = s

    def get_search_restriction_name(self):
        return self.search_restriction_name

    def set_search_restriction_name(self, s):
        self.search_restriction_name = s

    def search_restriction_applied(self):
        return bool(self.search_restriction) or bool(self.base_restriction)

    def get_search_restriction_book_count(self):
        return self.search_restriction_book_count

    def change_search_locations(self, newlocs):
        pass

    def set_marked_ids(self, id_dict):
        if not id_dict:
            self.marked_ids = {}
        else:
            self.marked_ids = dict.fromkeys(id_dict, 'true') if not isinstance(id_dict, dict) else dict(id_dict)

    def get_marked(self, book_id):
        return self.marked_ids.get(book_id)

    def add_marked_listener(self, func):
        pass

    def toggle_marked_ids(self, book_ids):
        for i in book_ids:
            if i in self.marked_ids:
                del self.marked_ids[i]
            else:
                self.marked_ids[i] = 'true'


__all__ = ['View', 'TableRow', 'sanitize_sort_field_name', 'sort_key']
