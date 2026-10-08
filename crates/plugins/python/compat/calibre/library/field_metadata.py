"""Mirrors calibre's `calibre.library.field_metadata` (calibre 9.15): the
description of every field in a library, standard and custom: its table,
datatype, whether it's multiple, how it's searched and shown.

Plugins read it as `db.field_metadata` (`fm['#pages']['datatype']`,
`fm.custom_field_keys()`, `fm.key_to_label()`, `fm.search_term_to_field_key()`).
"""

import copy
import traceback
from collections import OrderedDict


class TagsIcons(dict):
    """If the client wants icons to be in the tag structure, this class must be
    instantiated and filled in with real icons."""

    def __init__(self, icon_dict):
        for a in (
            'authors',
            'series',
            'formats',
            'publisher',
            'rating',
            'news',
            'tags',
            'custom:',
            'user:',
            'search',
            'identifiers',
            'gst',
        ):
            if a not in icon_dict:
                raise ValueError('Missing category icon [%s]' % a)
            self[a] = icon_dict[a]


category_icon_map = {
    'authors': 'user_profile.png',
    'series': 'series.png',
    'formats': 'book.png',
    'publisher': 'publisher.png',
    'rating': 'rating.png',
    'news': 'news.png',
    'tags': 'tags.png',
    'custom:': 'column.png',
    'user:': 'tb_folder.png',
    'search': 'search.png',
    'identifiers': 'identifiers.png',
    'gst': 'catalog.png',
    'languages': 'languages.png',
}


class FieldMetadata:
    """The field descriptions, keyed by field key ('title', '#pages'). Each
    description is a dict with 'table', 'column', 'datatype', 'is_multiple',
    'kind', 'name', 'search_terms', 'is_custom', 'is_category', 'is_csp',
    'is_editable', 'display', 'rec_index'/'colnum' (custom), 'label'
    (custom), 'link_column'."""

    VALID_DATA_TYPES = frozenset(
        ['datetime', 'enumeration', 'rating', 'text', 'bool', 'series', 'float', 'int', 'comments', 'composite', None]
    )

    # Builtin metadata {{{

    _field_metadata_prototype = [
        (
            'authors',
            {
                'table': 'authors',
                'column': 'name',
                'link_column': 'author',
                'category_sort': 'sort',
                'datatype': 'text',
                'is_multiple': {'cache_to_list': ',', 'ui_to_list': '&', 'list_to_ui': ' & '},
                'kind': 'field',
                'name': 'Authors',
                'search_terms': ['authors', 'author'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'languages',
            {
                'table': 'languages',
                'column': 'lang_code',
                'link_column': 'lang_code',
                'category_sort': 'lang_code',
                'datatype': 'text',
                'is_multiple': {'cache_to_list': ',', 'ui_to_list': ',', 'list_to_ui': ', '},
                'kind': 'field',
                'name': 'Languages',
                'search_terms': ['languages', 'language'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'series',
            {
                'table': 'series',
                'column': 'name',
                'link_column': 'series',
                'category_sort': '(title_sort(name))',
                'datatype': 'series',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Series',
                'search_terms': ['series'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'formats',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {'cache_to_list': ',', 'ui_to_list': ',', 'list_to_ui': ', '},
                'kind': 'field',
                'name': 'Formats',
                'search_terms': ['formats', 'format'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'publisher',
            {
                'table': 'publishers',
                'column': 'name',
                'link_column': 'publisher',
                'category_sort': 'name',
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Publisher',
                'search_terms': ['publisher'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'rating',
            {
                'table': 'ratings',
                'column': 'rating',
                'link_column': 'rating',
                'category_sort': 'rating',
                'datatype': 'rating',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Rating',
                'search_terms': ['rating'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'news',
            {
                'table': 'news',
                'column': 'name',
                'category_sort': 'name',
                'datatype': None,
                'is_multiple': {},
                'kind': 'category',
                'name': 'News',
                'search_terms': [],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'tags',
            {
                'table': 'tags',
                'column': 'name',
                'link_column': 'tag',
                'category_sort': 'name',
                'datatype': 'text',
                'is_multiple': {'cache_to_list': ',', 'ui_to_list': ',', 'list_to_ui': ', '},
                'kind': 'field',
                'name': 'Tags',
                'search_terms': ['tags', 'tag'],
                'is_custom': False,
                'is_category': True,
                'is_csp': False,
            },
        ),
        (
            'identifiers',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {'cache_to_list': ',', 'ui_to_list': ',', 'list_to_ui': ', '},
                'kind': 'field',
                'name': 'Identifiers',
                'search_terms': ['identifiers', 'identifier', 'isbn'],
                'is_custom': False,
                'is_category': True,
                'is_csp': True,
            },
        ),
        (
            'author_sort',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Author sort',
                'search_terms': ['author_sort'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'au_map',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {'cache_to_list': ',', 'ui_to_list': None, 'list_to_ui': None},
                'kind': 'field',
                'name': None,
                'search_terms': [],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'comments',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Comments',
                'search_terms': ['comments', 'comment'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'cover',
            {
                'table': None,
                'column': None,
                'datatype': 'int',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Cover',
                'search_terms': ['cover'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'id',
            {
                'table': None,
                'column': None,
                'datatype': 'int',
                'is_multiple': {},
                'kind': 'field',
                'name': None,
                'search_terms': ['id'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'last_modified',
            {
                'table': None,
                'column': None,
                'datatype': 'datetime',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Modified',
                'search_terms': ['last_modified'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'ondevice',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'On device',
                'search_terms': ['ondevice'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'path',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Path',
                'search_terms': [],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'pubdate',
            {
                'table': None,
                'column': None,
                'datatype': 'datetime',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Published',
                'search_terms': ['pubdate'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'marked',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': None,
                'search_terms': ['marked'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'series_index',
            {
                'table': None,
                'column': None,
                'datatype': 'float',
                'is_multiple': {},
                'kind': 'field',
                'name': None,
                'search_terms': ['series_index'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'series_sort',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Series sort',
                'search_terms': ['series_sort'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'sort',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Title sort',
                'search_terms': ['title_sort'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'size',
            {
                'table': None,
                'column': None,
                'datatype': 'float',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Size',
                'search_terms': ['size'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'timestamp',
            {
                'table': None,
                'column': None,
                'datatype': 'datetime',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Date',
                'search_terms': ['date'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'title',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': 'Title',
                'search_terms': ['title'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'uuid',
            {
                'table': None,
                'column': None,
                'datatype': 'text',
                'is_multiple': {},
                'kind': 'field',
                'name': None,
                'search_terms': ['uuid'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'in_tag_browser',
            {
                'table': None,
                'column': None,
                'datatype': None,
                'is_multiple': {},
                'kind': 'field',
                'name': None,
                'search_terms': ['in_tag_browser'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
        (
            'template',
            {
                'table': None,
                'column': None,
                'datatype': None,
                'is_multiple': {},
                'kind': 'field',
                'name': None,
                'search_terms': ['template'],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            },
        ),
    ]
    # }}}

    # search labels that are not db columns
    search_items = ['all', 'search', 'vl']
    __calibre_serializable__ = True

    def __init__(self):
        self._field_metadata = copy.deepcopy(self._field_metadata_prototype)
        self._tb_cats = OrderedDict()
        self._tb_custom_fields = {}
        self._search_term_map = {}
        self.custom_label_to_key_map = {}
        for k, v in self._field_metadata:
            if v['kind'] == 'field' and v['datatype'] not in self.VALID_DATA_TYPES:
                raise ValueError('Unknown datatype %s for field %s' % (v['datatype'], k))
            self._tb_cats[k] = v
            self._tb_cats[k]['label'] = k
            self._tb_cats[k]['display'] = {}
            self._tb_cats[k]['is_editable'] = True
            self._add_search_terms_to_map(k, v['search_terms'])
        self._tb_cats['timestamp']['display'] = {'date_format': 'dd MMM yyyy'}
        self._tb_cats['last_modified']['display'] = {'date_format': 'dd MMM yyyy'}
        self._tb_cats['pubdate']['display'] = {'date_format': 'MMM yyyy'}
        self.custom_field_prefix = '#'
        self.get = self._tb_cats.get

    def __getitem__(self, key):
        if key == 'title_sort':
            return self._tb_cats['sort']
        return self._tb_cats[key]

    def __setitem__(self, key, val):
        raise AttributeError('Assigning to this object is forbidden')

    def __delitem__(self, key):
        del self._tb_cats[key]

    def __iter__(self):
        yield from self._tb_cats

    def __contains__(self, key):
        return key in self._tb_cats or key == 'title_sort'

    def has_key(self, key):
        return key in self

    def keys(self):
        return self._tb_cats.keys()

    def __eq__(self, other):
        if not isinstance(other, FieldMetadata):
            return False
        for attr in ('_tb_cats', '_search_term_map', 'custom_label_to_key_map', 'custom_field_prefix'):
            if getattr(self, attr) != getattr(other, attr):
                return False
        return True

    def __ne__(self, other):
        return not self.__eq__(other)

    def sortable_field_keys(self):
        return [
            k
            for k in self._tb_cats.keys()
            if self._tb_cats[k]['kind'] == 'field' and self._tb_cats[k]['datatype'] is not None
        ]

    def ui_sortable_field_keys(self):
        ans = {
            k: self._tb_cats[k]['name']
            for k in set(self.sortable_field_keys())
            - {
                'sort',
                'author_sort',
                'au_map',
                'series_sort',
                'marked',
                'series_index',
                'path',
                'formats',
                'identifiers',
                'uuid',
                'comments',
            }
            if self._tb_cats[k]['name']
        }
        ans['cover'] = 'Cover presence'
        return ans

    def displayable_field_keys(self):
        return [
            k
            for k in self._tb_cats.keys()
            if self._tb_cats[k]['kind'] == 'field'
            and self._tb_cats[k]['datatype'] is not None
            and k not in ('au_map', 'marked', 'ondevice', 'cover', 'series_sort')
            and not self.is_series_index(k)
        ]

    def standard_field_keys(self):
        return [
            k for k in self._tb_cats.keys() if self._tb_cats[k]['kind'] == 'field' and not self._tb_cats[k]['is_custom']
        ]

    def custom_field_keys(self, include_composites=True):
        res = []
        for k in self._tb_cats.keys():
            fm = self._tb_cats[k]
            if fm['kind'] == 'field' and fm['is_custom'] and (include_composites or fm['datatype'] != 'composite'):
                res.append(k)
        return res

    def all_field_keys(self):
        return [k for k in self._tb_cats.keys() if self._tb_cats[k]['kind'] == 'field']

    def iterkeys(self):
        yield from self._tb_cats

    def itervalues(self):
        return iter(self._tb_cats.values())

    def values(self):
        return self._tb_cats.values()

    def iteritems(self):
        yield from self._tb_cats.items()

    def items(self):
        return self._tb_cats.items()

    def custom_iteritems(self):
        for key, meta in self._tb_cats.items():
            if meta['is_custom']:
                yield (key, meta)

    def is_custom_field(self, key):
        return key.startswith(self.custom_field_prefix)

    def is_ignorable_field(self, key):
        """Custom columns, user categories, and in the `ignore` list."""
        return self.is_custom_field(key) or key.startswith('@') or key in ('ondevice', 'search')

    def ignorable_field_keys(self):
        return [k for k in self._tb_cats if self.is_ignorable_field(k)]

    def is_series_index(self, key):
        try:
            m = self[key]
            return m['datatype'] == 'float' and key.endswith('_index') and key[:-6] in self._tb_cats
        except Exception:
            return False

    def key_to_label(self, key):
        if 'label' not in self._tb_cats[key]:
            return key
        return self._tb_cats[key]['label']

    def label_to_key(self, label, prefer_custom=False):
        if prefer_custom:
            if label in self.custom_label_to_key_map:
                return self.custom_label_to_key_map[label]
        if 'label' in self._tb_cats:
            return label
        if not prefer_custom:
            if label in self.custom_label_to_key_map:
                return self.custom_label_to_key_map[label]
        raise ValueError('Unknown key [%s]' % (label))

    def all_metadata(self):
        l = {}
        for k in self._tb_cats:
            l[k] = self._tb_cats[k]
        return l

    def custom_field_metadata(self, include_composites=True):
        if include_composites:
            return self._tb_custom_fields
        l = {}
        for k in self.custom_field_keys(include_composites):
            l[k] = self._tb_cats[k]
        return l

    def add_custom_field(
        self, label, table, column, datatype, colnum, name, display, is_editable, is_multiple, is_category, is_csp=False
    ):
        key = self.custom_field_prefix + label
        if key in self._tb_cats:
            raise ValueError('Duplicate custom field [%s]' % (label))
        if datatype not in self.VALID_DATA_TYPES:
            raise ValueError('Unknown datatype %s for field %s' % (datatype, key))
        self._tb_cats[key] = {
            'table': table,
            'column': column,
            'datatype': datatype,
            'is_multiple': is_multiple,
            'kind': 'field',
            'name': name,
            'search_terms': [key],
            'label': label,
            'colnum': colnum,
            'display': display,
            'is_custom': True,
            'is_category': is_category,
            'link_column': 'value',
            'category_sort': 'value',
            'is_csp': is_csp,
            'is_editable': is_editable,
        }
        self._tb_custom_fields[key] = self._tb_cats[key]
        self._add_search_terms_to_map(key, [key])
        self.custom_label_to_key_map[label] = key
        if datatype == 'series':
            key += '_index'
            self._tb_cats[key] = {
                'table': None,
                'column': None,
                'datatype': 'float',
                'is_multiple': {},
                'kind': 'field',
                'name': '',
                'search_terms': [key],
                'label': label + '_index',
                'colnum': None,
                'display': {},
                'is_custom': False,
                'is_category': False,
                'link_column': None,
                'category_sort': None,
                'is_editable': False,
                'is_csp': False,
            }
            self._add_search_terms_to_map(key, [key])
            self.custom_label_to_key_map[label + '_index'] = key

    def remove_custom_fields(self):
        for key in self.get_custom_fields():
            del self._tb_cats[key]

    def get_custom_fields(self):
        return [f for f in self._tb_cats if self._tb_cats[f]['is_custom']]

    def remove_dynamic_categories(self):
        for key in list(self._tb_cats.keys()):
            val = self._tb_cats[key]
            if val['is_category'] and val['kind'] in ('user', 'search'):
                del self._tb_cats[key]

    def cc_series_index_column_for(self, key):
        return self._tb_cats[key]['rec_index'] + 1

    def add_user_category(self, label, name):
        if label in self._tb_cats:
            raise ValueError('Duplicate user field [%s]' % (label))
        st = [label]
        if label.find('.') != -1:
            st.append(label.partition('.')[0])
        self._tb_cats[label] = {
            'table': None,
            'column': None,
            'datatype': None,
            'is_multiple': {},
            'kind': 'user',
            'name': name,
            'search_terms': st,
            'is_custom': False,
            'is_category': True,
            'is_csp': False,
        }
        self._add_search_terms_to_map(label, st)

    def add_search_category(self, label, name):
        if label in self._tb_cats:
            raise ValueError('Duplicate user field [%s]' % (label))
        self._tb_cats[label] = {
            'table': None,
            'column': None,
            'datatype': None,
            'is_multiple': {},
            'kind': 'search',
            'name': name,
            'search_terms': [],
            'is_custom': False,
            'is_category': True,
            'is_csp': False,
        }

    def add_grouped_search_terms(self, gst):
        from calibre.utils.icu import lower

        for t in gst:
            lt = lower(t)
            if lt in self._search_term_map:
                continue
            self._tb_cats[lt] = {
                'table': None,
                'column': None,
                'datatype': None,
                'is_multiple': {},
                'kind': 'search',
                'name': t,
                'search_terms': [lt],
                'is_custom': False,
                'is_category': False,
                'is_csp': False,
            }
            self._add_search_terms_to_map(lt, [lt])

    DEFAULT_LOCATIONS = frozenset(
        [
            'all',
            'author',
            'authors',
            'comments',
            'cover',
            'date',
            'format',
            'formats',
            'id',
            'identifier',
            'identifiers',
            'isbn',
            'languages',
            'last_modified',
            'ondevice',
            'pubdate',
            'publisher',
            'rating',
            'search',
            'series',
            'series_index',
            'series_sort',
            'size',
            'tag',
            'tags',
            'template',
            'title',
            'title_sort',
            'uuid',
            'vl',
        ]
    )

    def get_search_terms(self):
        s_keys = sorted(self._search_term_map.keys())
        for v in self.search_items:
            s_keys.append(v)
        return s_keys

    def _add_search_terms_to_map(self, key, terms):
        from calibre.utils.icu import lower

        if terms is not None:
            for t in terms:
                t = lower(t)
                if t in self._search_term_map:
                    raise ValueError('Attempt to add duplicate search term "%s"' % t)
                self._search_term_map[t] = key

    def search_term_to_field_key(self, term):
        return self._search_term_map.get(term, term)

    def searchable_fields(self):
        return [
            k
            for k in self._tb_cats.keys()
            if self._tb_cats[k]['kind'] == 'field' and len(self._tb_cats[k]['search_terms']) > 0
        ]

    def field_keys(self):
        return list(self._tb_cats.keys())


def fm_as_dict(self):
    return {
        'fm': self._tb_cats,
        'custom': self._tb_custom_fields,
        'search_terms': self._search_term_map,
        'label_map': self.custom_label_to_key_map,
    }


def fm_from_dict(src):
    ans = FieldMetadata()
    ans._tb_cats = OrderedDict(src['fm'])
    ans._tb_custom_fields = src['custom']
    ans._search_term_map = src['search_terms']
    ans.custom_label_to_key_map = src['label_map']
    ans.get = ans._tb_cats.get
    return ans


__all__ = ['FieldMetadata', 'TagsIcons', 'category_icon_map', 'fm_as_dict', 'fm_from_dict', 'traceback']
